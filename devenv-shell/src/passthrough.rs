//! Byte-stream rewrite for alternate-screen passthrough.
//!
//! When a full-screen application runs on the alternate screen, devenv can
//! forward the application's output straight to the real terminal instead of
//! re-rendering it through the VT (which costs a per-cell read of every changed
//! row each frame). The application's PTY is sized one row shorter than the real
//! terminal — the bottom row is reserved for devenv's status line — so the
//! application never *addresses* the status row. But it can still *scroll over*
//! it if it resets its scroll region to the full screen, and it can be told the
//! wrong height if it queries the terminal directly. This filter rewrites the
//! forwarded stream so the reserved bottom row stays protected:
//!
//! - A scroll-region reset (`CSI r`, no params) becomes an explicit
//!   `CSI 1 ; content_rows r`, so a reset cannot expose the status row.
//! - Entering the alternate screen (`CSI ? 1049 h` / `47h` / `1047h`) injects
//!   the same explicit scroll region afterwards, in case the app relies on the
//!   default region rather than setting its own.
//! - The text-area-size query (`CSI 18 t`) is dropped, because the host answers
//!   it with the reserved (one-row-shorter) size; letting the real terminal
//!   answer too would report the full height and invite the app to draw into
//!   the status row.
//!
//! Everything else passes through verbatim: content, styling, cursor moves,
//! the app's own explicit scroll regions (already within its shorter height),
//! and OSC/DCS strings. State is carried across calls so escape sequences split
//! across reads are still rewritten correctly.

/// Alternate-screen modes whose entry should (re)assert the reserved scroll
/// region: 1049 (save+alt+clear), 1047 (alt), 47 (legacy alt).
const ALT_SCREEN_MODES: [u16; 3] = [1049, 1047, 47];

/// A malformed/oversized CSI sequence is flushed verbatim once it exceeds this
/// many bytes, so a stray `ESC [` in the stream cannot make the filter buffer
/// unboundedly.
const MAX_CSI_LEN: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Ground,
    Esc,
    Csi,
    Osc,
    OscEsc,
    Dcs,
    DcsEsc,
}

/// Stateful rewriter for one passthrough session. Reused across PTY reads.
pub struct PassthroughFilter {
    state: State,
    /// Bytes of the CSI sequence currently being accumulated (includes the
    /// leading `ESC [`). Only CSI sequences are buffered, since they are the
    /// only ones rewritten; all other bytes are copied straight to the output.
    csi: Vec<u8>,
}

impl PassthroughFilter {
    pub fn new() -> Self {
        Self {
            state: State::Ground,
            csi: Vec::new(),
        }
    }

    /// Reset to the ground state, discarding any partially-buffered sequence.
    /// Called when passthrough is (re)entered so a sequence left dangling by a
    /// previous excursion cannot leak into a new one.
    pub fn reset(&mut self) {
        self.state = State::Ground;
        self.csi.clear();
    }

    /// Append the rewritten form of `data` to `out`. `content_rows` is the
    /// number of rows available to the app (the real terminal height minus the
    /// reserved status row).
    pub fn rewrite(&mut self, data: &[u8], content_rows: u16, out: &mut Vec<u8>) {
        for &b in data {
            match self.state {
                State::Ground => {
                    if b == 0x1b {
                        self.state = State::Esc;
                    } else {
                        out.push(b);
                    }
                }
                State::Esc => match b {
                    b'[' => {
                        self.csi.clear();
                        self.csi.extend_from_slice(b"\x1b[");
                        self.state = State::Csi;
                    }
                    b']' => {
                        out.extend_from_slice(b"\x1b]");
                        self.state = State::Osc;
                    }
                    b'P' => {
                        out.extend_from_slice(b"\x1bP");
                        self.state = State::Dcs;
                    }
                    0x1b => {
                        // Another ESC restarts; emit the one we held back.
                        out.push(0x1b);
                    }
                    _ => {
                        // Two-byte escape (e.g. ESC =, ESC >, ESC c): verbatim.
                        out.push(0x1b);
                        out.push(b);
                        self.state = State::Ground;
                    }
                },
                State::Csi => {
                    if b == 0x1b {
                        // Abort the partial CSI (matches terminal behavior) and
                        // start a new sequence.
                        self.csi.clear();
                        self.state = State::Esc;
                    } else {
                        self.csi.push(b);
                        if (0x40..=0x7e).contains(&b) {
                            // Final byte: the CSI is complete.
                            rewrite_csi(&self.csi, content_rows, out);
                            self.csi.clear();
                            self.state = State::Ground;
                        } else if !(0x20..=0x3f).contains(&b) || self.csi.len() > MAX_CSI_LEN {
                            // Invalid intermediate/param byte or runaway length:
                            // emit what we have verbatim and resync to ground.
                            out.extend_from_slice(&self.csi);
                            self.csi.clear();
                            self.state = State::Ground;
                        }
                    }
                }
                State::Osc => {
                    out.push(b);
                    match b {
                        0x07 => self.state = State::Ground, // BEL terminator
                        0x1b => self.state = State::OscEsc,
                        _ => {}
                    }
                }
                State::OscEsc => {
                    // ESC \ is the ST terminator; anything else, best-effort
                    // return to ground (OSC payloads do not contain bare ESC).
                    out.push(b);
                    self.state = State::Ground;
                }
                State::Dcs => {
                    out.push(b);
                    if b == 0x1b {
                        self.state = State::DcsEsc;
                    }
                }
                State::DcsEsc => {
                    out.push(b);
                    self.state = State::Ground;
                }
            }
        }
    }
}

impl Default for PassthroughFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// Append the rewritten form of one complete CSI sequence (`seq`, including the
/// leading `ESC [`) to `out`.
fn rewrite_csi(seq: &[u8], content_rows: u16, out: &mut Vec<u8>) {
    // seq is `ESC [ <params/intermediates> <final>`; len >= 3.
    let final_byte = seq[seq.len() - 1];
    let params = &seq[2..seq.len() - 1];

    // Scroll-region reset (`CSI r`) → explicit region protecting the status row.
    if final_byte == b'r' && params.is_empty() {
        push_scroll_region(content_rows, out);
        return;
    }

    // Text-area-size query (`CSI 18 t`) → drop; the host answers it instead.
    if final_byte == b't' && params == b"18" {
        return;
    }

    // Entering the alternate screen (`CSI ? <modes> h`) → verbatim, then assert
    // the reserved scroll region in case the app relies on the default.
    if final_byte == b'h' && params.first() == Some(&b'?') && mode_list_has_alt(&params[1..]) {
        out.extend_from_slice(seq);
        push_scroll_region(content_rows, out);
        return;
    }

    out.extend_from_slice(seq);
}

/// Whether a `;`-separated DEC mode parameter list contains an alt-screen mode.
fn mode_list_has_alt(modes: &[u8]) -> bool {
    modes
        .split(|&b| b == b';')
        .filter_map(|m| std::str::from_utf8(m).ok()?.parse::<u16>().ok())
        .any(|m| ALT_SCREEN_MODES.contains(&m))
}

fn push_scroll_region(content_rows: u16, out: &mut Vec<u8>) {
    out.extend_from_slice(b"\x1b[1;");
    let mut buf = itoa_u16(content_rows);
    out.append(&mut buf);
    out.push(b'r');
}

/// Decimal-encode a `u16` without pulling in a formatting allocation per call
/// site (the value is at most 5 digits).
fn itoa_u16(mut n: u16) -> Vec<u8> {
    if n == 0 {
        return vec![b'0'];
    }
    let mut digits = Vec::with_capacity(5);
    while n > 0 {
        digits.push(b'0' + (n % 10) as u8);
        n /= 10;
    }
    digits.reverse();
    digits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(filter: &mut PassthroughFilter, data: &[u8], rows: u16) -> Vec<u8> {
        let mut out = Vec::new();
        filter.rewrite(data, rows, &mut out);
        out
    }

    #[test]
    fn plain_content_passes_through() {
        let mut f = PassthroughFilter::new();
        assert_eq!(run(&mut f, b"hello world\r\n", 24), b"hello world\r\n");
    }

    #[test]
    fn bare_scroll_region_reset_is_clamped() {
        let mut f = PassthroughFilter::new();
        // CSI r (reset to full screen) must become CSI 1;24 r so a full-screen
        // app cannot scroll into the reserved status row (row 25).
        assert_eq!(run(&mut f, b"\x1b[r", 24), b"\x1b[1;24r");
    }

    #[test]
    fn explicit_scroll_region_passes_through() {
        let mut f = PassthroughFilter::new();
        // The app's PTY is already one row shorter, so an explicit region is
        // within bounds and must not be touched.
        assert_eq!(run(&mut f, b"\x1b[1;20r", 24), b"\x1b[1;20r");
        assert_eq!(run(&mut f, b"\x1b[5;18r", 24), b"\x1b[5;18r");
    }

    #[test]
    fn text_area_size_query_is_stripped() {
        let mut f = PassthroughFilter::new();
        assert_eq!(run(&mut f, b"a\x1b[18tb", 24), b"ab");
    }

    #[test]
    fn other_xtwinops_pass_through() {
        let mut f = PassthroughFilter::new();
        // CSI 14 t (text area in pixels) is not the one we intercept.
        assert_eq!(run(&mut f, b"\x1b[14t", 24), b"\x1b[14t");
    }

    #[test]
    fn alt_screen_enter_injects_scroll_region() {
        let mut f = PassthroughFilter::new();
        assert_eq!(run(&mut f, b"\x1b[?1049h", 24), b"\x1b[?1049h\x1b[1;24r");
    }

    #[test]
    fn alt_screen_enter_compound_modes_injects_once() {
        let mut f = PassthroughFilter::new();
        assert_eq!(
            run(&mut f, b"\x1b[?1049;1006h", 24),
            b"\x1b[?1049;1006h\x1b[1;24r"
        );
    }

    #[test]
    fn alt_screen_legacy_variants() {
        for mode in ["47", "1047"] {
            let mut f = PassthroughFilter::new();
            let input = format!("\x1b[?{mode}h");
            let expected = format!("\x1b[?{mode}h\x1b[1;24r");
            assert_eq!(run(&mut f, input.as_bytes(), 24), expected.as_bytes());
        }
    }

    #[test]
    fn alt_screen_leave_passes_through() {
        let mut f = PassthroughFilter::new();
        // Leaving the alt screen must not inject a region.
        assert_eq!(run(&mut f, b"\x1b[?1049l", 24), b"\x1b[?1049l");
    }

    #[test]
    fn unrelated_dec_mode_passes_through() {
        let mut f = PassthroughFilter::new();
        // Mouse tracking, bracketed paste, etc. are not alt-screen modes.
        assert_eq!(run(&mut f, b"\x1b[?1000h", 24), b"\x1b[?1000h");
        assert_eq!(run(&mut f, b"\x1b[?2004h", 24), b"\x1b[?2004h");
    }

    #[test]
    fn sgr_and_cursor_moves_pass_through() {
        let mut f = PassthroughFilter::new();
        let input = b"\x1b[1;31m\x1b[10;5HX\x1b[0m";
        assert_eq!(run(&mut f, input, 24), input);
    }

    #[test]
    fn osc_payload_is_not_misread_as_csi() {
        let mut f = PassthroughFilter::new();
        // An OSC title containing "[r" / "[18t"-looking bytes must pass verbatim.
        let input = b"\x1b]0;weird [r [18t title\x07rest";
        assert_eq!(run(&mut f, input, 24), input);
    }

    #[test]
    fn osc_with_st_terminator() {
        let mut f = PassthroughFilter::new();
        let input = b"\x1b]8;;http://x\x1b\\link";
        assert_eq!(run(&mut f, input, 24), input);
    }

    #[test]
    fn dcs_payload_passes_through() {
        let mut f = PassthroughFilter::new();
        let input = b"\x1bP1$r0m\x1b\\after";
        assert_eq!(run(&mut f, input, 24), input);
    }

    #[test]
    fn csi_split_across_reads_is_rewritten() {
        let mut f = PassthroughFilter::new();
        let mut out = Vec::new();
        // `CSI r` arriving as "ESC", "[", "r" in three reads.
        f.rewrite(b"\x1b", 24, &mut out);
        f.rewrite(b"[", 24, &mut out);
        f.rewrite(b"r", 24, &mut out);
        assert_eq!(out, b"\x1b[1;24r");
    }

    #[test]
    fn alt_enter_split_across_reads() {
        let mut f = PassthroughFilter::new();
        let mut out = Vec::new();
        f.rewrite(b"\x1b[?10", 24, &mut out);
        f.rewrite(b"49h", 24, &mut out);
        assert_eq!(out, b"\x1b[?1049h\x1b[1;24r");
    }

    #[test]
    fn content_around_sequences_is_preserved() {
        let mut f = PassthroughFilter::new();
        let out = run(&mut f, b"before\x1b[rmiddle\x1b[18tafter", 30);
        assert_eq!(out, b"before\x1b[1;30rmiddleafter");
    }

    #[test]
    fn reset_clears_dangling_state() {
        let mut f = PassthroughFilter::new();
        let mut out = Vec::new();
        f.rewrite(b"\x1b[12", 24, &mut out); // partial CSI left dangling
        f.reset();
        // After reset, a fresh content byte is emitted as ground, not appended
        // to the abandoned CSI.
        out.clear();
        f.rewrite(b"X", 24, &mut out);
        assert_eq!(out, b"X");
    }
}
