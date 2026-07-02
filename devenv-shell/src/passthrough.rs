//! Byte-stream rewrite for alternate-screen passthrough.
//!
//! When a full-screen application runs on the alternate screen, devenv can
//! forward the application's output straight to the real terminal instead of
//! re-rendering it through the VT (which costs a per-cell read of every changed
//! row each frame). The application's PTY is sized one row shorter than the real
//! terminal — the bottom row is reserved for devenv's status line — so the
//! application never knowingly *addresses* the status row. But it can still
//! scroll over it, address it via clamped absolute moves, or be told the wrong
//! height if it queries the terminal directly. This filter rewrites the
//! forwarded stream so the reserved bottom row stays protected:
//!
//! - Any DECSTBM (`CSI top;bottom r`) whose bottom margin is omitted, zero, or
//!   beyond the app's height is clamped to `content_rows`, so no scroll-region
//!   form can expose the status row.
//! - Absolute row addressing (`CSI row;col H`/`f`, `CSI row d`) beyond the
//!   app's height is clamped to `content_rows`; the real terminal would clamp
//!   it to its own (one-row-taller) bottom, i.e. the status row — this also
//!   keeps the `CSI 999;999H` + DSR-6 size probe from reporting the full
//!   height.
//! - Entering the alternate screen (`CSI ? 1049 h` / `47h` / `1047h`) injects
//!   the reserved scroll region afterwards, in case the app relies on the
//!   default region rather than setting its own; DECSTR (`CSI !p`) and RIS
//!   (`ESC c`) reset the margins, so the region is re-asserted after them too.
//! - The size queries `CSI 18 t` (cells; the host answers it with the reserved
//!   size) and `CSI 14 t` (pixels; never forwarded by the mediated path either)
//!   are dropped, so the real terminal cannot report the unreserved height.
//! - Mode 2048 (in-band resize) is stripped from DEC mode lists: the host
//!   sends its own in-band reports with the reserved size, while the real
//!   terminal would immediately report its full height.
//!
//! Everything else passes through verbatim: content, styling, cursor moves,
//! in-bounds scroll regions, and OSC/DCS strings.
//!
//! The output is *boundary-clean*: `rewrite` never leaves `out` ending inside
//! an escape sequence or a multi-byte UTF-8 character. CSI sequences, OSC/DCS
//! strings, and partial codepoints split across reads are buffered and emitted
//! only once complete, so the caller can safely interleave its own writes
//! (status line, cursor moves) between `rewrite` calls. The only exception is
//! a string sequence larger than [`MAX_STRING_LEN`], which is flushed in
//! chunks to bound memory.

use crate::escape::{ALT_SCREEN_MODES, IN_BAND_RESIZE_MODE};
use std::io::Write;

/// A malformed/oversized CSI sequence is flushed verbatim once it exceeds this
/// many bytes, so a stray `ESC [` in the stream cannot make the filter buffer
/// unboundedly.
const MAX_CSI_LEN: usize = 64;

/// OSC/DCS strings are buffered until their terminator so host writes are
/// never injected mid-string; payloads beyond this size (huge OSC 52
/// clipboards, sixel images) are flushed in chunks to bound memory, giving up
/// the boundary guarantee only for those.
const MAX_STRING_LEN: usize = 1 << 20;

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

/// Stateful rewriter for one shell session. Fed every PTY read (the session
/// discards the output for mediated batches) so its parse state stays aligned
/// with the byte stream across mediated/passthrough handoffs — a sequence
/// split across the handoff is emitted whole instead of leaking a raw tail.
pub struct PassthroughFilter {
    state: State,
    /// Bytes of the CSI sequence currently being accumulated (includes the
    /// leading `ESC [`).
    csi: Vec<u8>,
    /// Bytes of the OSC/DCS string currently being accumulated (includes the
    /// leading `ESC ]` / `ESC P`).
    string_buf: Vec<u8>,
    /// Partial UTF-8 codepoint held back until its continuation bytes arrive.
    utf8: [u8; 4],
    utf8_len: u8,
    utf8_need: u8,
}

impl PassthroughFilter {
    pub fn new() -> Self {
        Self {
            state: State::Ground,
            csi: Vec::new(),
            string_buf: Vec::new(),
            utf8: [0; 4],
            utf8_len: 0,
            utf8_need: 0,
        }
    }

    /// Append the rewritten form of `data` to `out`. `content_rows` is the
    /// number of rows available to the app (the real terminal height minus the
    /// reserved status row).
    ///
    /// The Ground and OSC/DCS states copy runs of uninteresting bytes in bulk;
    /// this runs on every PTY read (mediated batches included, for handoff
    /// continuity), so plain output must cost a scan, not a per-byte loop.
    pub fn rewrite(&mut self, data: &[u8], content_rows: u16, out: &mut Vec<u8>) {
        let n = data.len();
        let mut i = 0;
        while i < n {
            let b = data[i];
            match self.state {
                State::Ground => {
                    if self.utf8_need > 0 {
                        if (0x80..=0xbf).contains(&b) {
                            self.utf8[self.utf8_len as usize] = b;
                            self.utf8_len += 1;
                            if self.utf8_len == self.utf8_need {
                                out.extend_from_slice(&self.utf8[..self.utf8_len as usize]);
                                self.utf8_len = 0;
                                self.utf8_need = 0;
                            }
                            i += 1;
                            continue;
                        }
                        // Malformed continuation: flush what was held and
                        // process the byte normally.
                        out.extend_from_slice(&self.utf8[..self.utf8_len as usize]);
                        self.utf8_len = 0;
                        self.utf8_need = 0;
                    }
                    // Bulk-copy the run of plain bytes up to the next ESC.
                    // Complete multi-byte UTF-8 codepoints stay inside the
                    // run (their continuations are verified so a malformed
                    // sequence cannot swallow an ESC); only a codepoint split
                    // at the end of `data` is held back, so host writes
                    // interleaved between reads can never split it on the
                    // real terminal.
                    let start = i;
                    loop {
                        while i < n && data[i] != 0x1b && !(0xc2..=0xf4).contains(&data[i]) {
                            i += 1;
                        }
                        if i == n || data[i] == 0x1b {
                            break;
                        }
                        let need = Self::utf8_need_of(data[i]);
                        if i + need <= n
                            && data[i + 1..i + need]
                                .iter()
                                .all(|c| (0x80..=0xbf).contains(c))
                        {
                            i += need;
                            continue;
                        }
                        break;
                    }
                    out.extend_from_slice(&data[start..i]);
                    if i == n {
                        break;
                    }
                    let b = data[i];
                    i += 1;
                    if b == 0x1b {
                        self.state = State::Esc;
                    } else {
                        let need = Self::utf8_need_of(b);
                        if i + need - 1 <= n {
                            // Malformed lead (the scan above rejected it):
                            // emit it verbatim and rescan from the next byte.
                            out.push(b);
                        } else {
                            self.begin_utf8(b, need as u8);
                        }
                    }
                    continue;
                }
                State::Esc => match b {
                    b'[' => {
                        self.csi.clear();
                        self.csi.extend_from_slice(b"\x1b[");
                        self.state = State::Csi;
                    }
                    b']' => {
                        self.string_buf.clear();
                        self.string_buf.extend_from_slice(b"\x1b]");
                        self.state = State::Osc;
                    }
                    b'P' => {
                        self.string_buf.clear();
                        self.string_buf.extend_from_slice(b"\x1bP");
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
                        if b == b'c' {
                            // RIS (full reset) clears the scroll margins;
                            // re-assert the reserved region right after.
                            push_scroll_region(content_rows, out);
                        }
                        self.state = State::Ground;
                    }
                },
                State::Csi => {
                    // Bulk-collect parameter and intermediate bytes.
                    let start = i;
                    while i < n && (0x20..=0x3f).contains(&data[i]) {
                        i += 1;
                    }
                    self.csi.extend_from_slice(&data[start..i]);
                    if self.csi.len() > MAX_CSI_LEN {
                        // Runaway length: emit verbatim and resync to ground.
                        out.extend_from_slice(&self.csi);
                        self.csi.clear();
                        self.state = State::Ground;
                        continue;
                    }
                    if i == n {
                        break;
                    }
                    let b = data[i];
                    i += 1;
                    if (0x40..=0x7e).contains(&b) {
                        // Final byte: the CSI is complete.
                        self.csi.push(b);
                        rewrite_csi(&self.csi, content_rows, out);
                        self.csi.clear();
                        self.state = State::Ground;
                    } else if b == 0x1b {
                        // Abort the partial CSI (matches terminal behavior) and
                        // start a new sequence.
                        self.csi.clear();
                        self.state = State::Esc;
                    } else if b == 0x18 || b == 0x1a {
                        // CAN/SUB abort the sequence; the buffered bytes are
                        // dropped (the terminal never executes them).
                        self.csi.clear();
                        out.push(b);
                        self.state = State::Ground;
                    } else if b < 0x20 {
                        // C0 controls inside a CSI are executed immediately
                        // while the sequence continues (VT500 semantics).
                        // Emit the control now; the CSI follows when complete.
                        out.push(b);
                    } else if b == 0x7f {
                        // DEL is ignored inside a CSI.
                    } else {
                        // Invalid byte: emit what we have verbatim and resync
                        // to ground.
                        self.csi.push(b);
                        out.extend_from_slice(&self.csi);
                        self.csi.clear();
                        self.state = State::Ground;
                    }
                    continue;
                }
                State::Osc => {
                    // Bulk-buffer the payload up to the next BEL or ESC.
                    let start = i;
                    while i < n && data[i] != 0x07 && data[i] != 0x1b {
                        i += 1;
                    }
                    self.string_buf.extend_from_slice(&data[start..i]);
                    if i == n {
                        self.spill_oversized_string(out);
                        break;
                    }
                    self.string_buf.push(data[i]);
                    match data[i] {
                        0x07 => self.flush_string(out), // BEL terminator
                        _ => self.state = State::OscEsc,
                    }
                    i += 1;
                    continue;
                }
                State::OscEsc => {
                    // ESC \ is the ST terminator; anything else aborted the
                    // string (OSC payloads do not contain bare ESC). Either
                    // way the buffered bytes are flushed verbatim.
                    self.string_buf.push(b);
                    self.flush_string(out);
                }
                State::Dcs => {
                    // Bulk-buffer the payload up to the next ESC.
                    let start = i;
                    while i < n && data[i] != 0x1b {
                        i += 1;
                    }
                    self.string_buf.extend_from_slice(&data[start..i]);
                    if i == n {
                        self.spill_oversized_string(out);
                        break;
                    }
                    self.string_buf.push(0x1b);
                    self.state = State::DcsEsc;
                    i += 1;
                    continue;
                }
                State::DcsEsc => {
                    self.string_buf.push(b);
                    self.flush_string(out);
                }
            }
            i += 1;
        }
    }

    /// Expected byte length of a UTF-8 sequence with lead byte `b`
    /// (`0xc2..=0xf4`).
    fn utf8_need_of(b: u8) -> usize {
        match b {
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            _ => 4,
        }
    }

    fn begin_utf8(&mut self, lead: u8, need: u8) {
        self.utf8[0] = lead;
        self.utf8_len = 1;
        self.utf8_need = need;
    }

    /// Emit the buffered OSC/DCS string and return to ground.
    fn flush_string(&mut self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.string_buf);
        self.string_buf.clear();
        self.state = State::Ground;
    }

    /// Flush the buffered string prefix once it exceeds [`MAX_STRING_LEN`],
    /// keeping the parse state. Bounds memory for huge payloads at the cost
    /// of the boundary guarantee for them.
    fn spill_oversized_string(&mut self, out: &mut Vec<u8>) {
        if self.string_buf.len() > MAX_STRING_LEN {
            out.extend_from_slice(&self.string_buf);
            self.string_buf.clear();
        }
    }
}

impl Default for PassthroughFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse `;`-separated numeric CSI parameters. Empty entries are `None`
/// (default). Returns `None` if any entry is non-numeric (colon subparams,
/// garbage) — callers then pass the sequence through verbatim.
fn parse_params(params: &[u8]) -> Option<Vec<Option<u16>>> {
    if params.is_empty() {
        return Some(Vec::new());
    }
    params
        .split(|&b| b == b';')
        .map(|p| {
            if p.is_empty() {
                Some(None)
            } else {
                std::str::from_utf8(p).ok()?.parse::<u16>().ok().map(Some)
            }
        })
        .collect()
}

/// Read parameter `idx`, treating omitted and `0` as the given default.
fn param_or(params: &[Option<u16>], idx: usize, default: u16) -> u16 {
    params
        .get(idx)
        .copied()
        .flatten()
        .filter(|&v| v > 0)
        .unwrap_or(default)
}

/// Append the rewritten form of one complete CSI sequence (`seq`, including the
/// leading `ESC [`) to `out`.
fn rewrite_csi(seq: &[u8], content_rows: u16, out: &mut Vec<u8>) {
    // seq is `ESC [ <marker?> <params> <intermediates> <final>`; len >= 3.
    let final_byte = seq[seq.len() - 1];
    let body = &seq[2..seq.len() - 1];
    let (marker, rest) = match body.first() {
        Some(&m @ (b'?' | b'<' | b'=' | b'>')) => (Some(m), &body[1..]),
        _ => (None, body),
    };
    let inter_start = rest
        .iter()
        .position(|b| (0x20..=0x2f).contains(b))
        .unwrap_or(rest.len());
    let (param_bytes, intermediates) = rest.split_at(inter_start);

    match (marker, intermediates, final_byte) {
        // DECSTBM. The real terminal resolves an omitted/zero bottom margin to
        // its own (one-row-taller) height, so every form is normalized with
        // the bottom clamped to the app's height.
        (None, [], b'r') => {
            let Some(p) = parse_params(param_bytes) else {
                out.extend_from_slice(seq);
                return;
            };
            let top = param_or(&p, 0, 1);
            let bottom = param_or(&p, 1, content_rows).min(content_rows);
            if top < bottom {
                let _ = write!(out, "\x1b[{top};{bottom}r");
            } else {
                // Degenerate after clamping: fall back to the full protected
                // region rather than forwarding a region the terminal would
                // resolve against its taller height.
                push_scroll_region(content_rows, out);
            }
        }

        // XTWINOPS size queries. 18 (cells) is answered by the host with the
        // reserved size; 14 (pixels) is not forwarded by the mediated path
        // either — the real terminal's answer would describe the unreserved
        // full-height text area.
        (None, [], b't') => {
            let Some(p) = parse_params(param_bytes) else {
                out.extend_from_slice(seq);
                return;
            };
            match p.first().copied().flatten() {
                Some(14) | Some(18) => {}
                _ => out.extend_from_slice(seq),
            }
        }

        // CUP/HVP: absolute addressing is not confined by DECSTBM, so a row
        // beyond the app's height (e.g. the `CSI 999;999H` size probe) would
        // clamp onto the real terminal's bottom row — the status row.
        (None, [], b'H' | b'f') => {
            let Some(p) = parse_params(param_bytes) else {
                out.extend_from_slice(seq);
                return;
            };
            if param_or(&p, 0, 1) > content_rows {
                let col = param_or(&p, 1, 1);
                let _ = write!(out, "\x1b[{content_rows};{col}");
                out.push(final_byte);
            } else {
                out.extend_from_slice(seq);
            }
        }

        // VPA: same clamp as CUP for the row-only form.
        (None, [], b'd') => {
            let Some(p) = parse_params(param_bytes) else {
                out.extend_from_slice(seq);
                return;
            };
            if param_or(&p, 0, 1) > content_rows {
                let _ = write!(out, "\x1b[{content_rows}d");
            } else {
                out.extend_from_slice(seq);
            }
        }

        // DEC private mode set/reset. Mode 2048 (in-band resize) is stripped:
        // the host sends its own reports with the reserved size, while the
        // real terminal would immediately report its full height. Entering
        // the alternate screen asserts the reserved scroll region, in case
        // the app relies on the default region.
        (Some(b'?'), [], b'h' | b'l') => {
            let Some(p) = parse_params(param_bytes) else {
                out.extend_from_slice(seq);
                return;
            };
            let modes: Vec<u16> = p.into_iter().flatten().collect();
            let kept: Vec<u16> = modes
                .iter()
                .copied()
                .filter(|&m| m != IN_BAND_RESIZE_MODE)
                .collect();
            if kept.len() == modes.len() {
                out.extend_from_slice(seq);
            } else if !kept.is_empty() {
                out.extend_from_slice(b"\x1b[?");
                for (i, m) in kept.iter().enumerate() {
                    if i > 0 {
                        out.push(b';');
                    }
                    let _ = write!(out, "{m}");
                }
                out.push(final_byte);
            }
            if final_byte == b'h' && kept.iter().any(|m| ALT_SCREEN_MODES.contains(m)) {
                push_scroll_region(content_rows, out);
            }
        }

        // DECSTR (soft reset) clears the scroll margins; re-assert the
        // reserved region right after.
        (None, [b'!'], b'p') => {
            out.extend_from_slice(seq);
            push_scroll_region(content_rows, out);
        }

        _ => out.extend_from_slice(seq),
    }
}

fn push_scroll_region(content_rows: u16, out: &mut Vec<u8>) {
    // Vec<u8>'s io::Write never fails.
    let _ = write!(out, "\x1b[1;{content_rows}r");
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
    fn default_param_scroll_region_forms_are_clamped() {
        // `CSI 0;0r`, `CSI ;r` and `CSI 0r` are semantically identical to a
        // bare reset; the real terminal resolves the defaulted bottom to its
        // full (one-row-taller) height.
        for input in [&b"\x1b[0;0r"[..], b"\x1b[;r", b"\x1b[0r"] {
            let mut f = PassthroughFilter::new();
            assert_eq!(run(&mut f, input, 24), b"\x1b[1;24r", "input {input:?}");
        }
    }

    #[test]
    fn omitted_or_oversized_bottom_margin_is_clamped() {
        let mut f = PassthroughFilter::new();
        // Top-only DECSTBM: the bottom defaults to the page's last line.
        assert_eq!(run(&mut f, b"\x1b[5r", 24), b"\x1b[5;24r");
        // Bottom beyond the app's height clamps to it.
        assert_eq!(run(&mut f, b"\x1b[1;999r", 24), b"\x1b[1;24r");
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
    fn dec_mode_restore_is_not_mistaken_for_decstbm() {
        let mut f = PassthroughFilter::new();
        // XTRESTORE (`CSI ? ... r`) restores DEC modes; it is not a scroll
        // region and must pass verbatim.
        assert_eq!(run(&mut f, b"\x1b[?1049r", 24), b"\x1b[?1049r");
    }

    #[test]
    fn text_area_size_query_is_stripped() {
        let mut f = PassthroughFilter::new();
        assert_eq!(run(&mut f, b"a\x1b[18tb", 24), b"ab");
        // Leading zeros are still the same query.
        assert_eq!(run(&mut f, b"\x1b[018t", 24), b"");
    }

    #[test]
    fn pixel_size_query_is_stripped() {
        let mut f = PassthroughFilter::new();
        // The mediated path never forwards CSI 14 t; the real terminal's
        // answer would describe the full-height text area.
        assert_eq!(run(&mut f, b"\x1b[14t", 24), b"");
    }

    #[test]
    fn other_xtwinops_pass_through() {
        let mut f = PassthroughFilter::new();
        // Cell size (16) and title query (21) are forwarded in mediated mode
        // too.
        assert_eq!(run(&mut f, b"\x1b[16t", 24), b"\x1b[16t");
        assert_eq!(run(&mut f, b"\x1b[21t", 24), b"\x1b[21t");
    }

    #[test]
    fn absolute_row_addressing_is_clamped() {
        let mut f = PassthroughFilter::new();
        // The `CSI 999;999H` size probe would land the cursor on the real
        // terminal's bottom row (the status row) and make a DSR-6 report the
        // unreserved height.
        assert_eq!(run(&mut f, b"\x1b[999;999H", 24), b"\x1b[24;999H");
        assert_eq!(run(&mut f, b"\x1b[999d", 24), b"\x1b[24d");
        assert_eq!(run(&mut f, b"\x1b[30;1f", 24), b"\x1b[24;1f");
        // In-bounds addressing is untouched, byte for byte.
        assert_eq!(run(&mut f, b"\x1b[10;5H", 24), b"\x1b[10;5H");
        assert_eq!(run(&mut f, b"\x1b[24;80H", 24), b"\x1b[24;80H");
    }

    #[test]
    fn in_band_resize_mode_is_stripped() {
        let mut f = PassthroughFilter::new();
        // The real terminal would answer ?2048h with an in-band report of its
        // full height; the host sends its own reports with the PTY size.
        assert_eq!(run(&mut f, b"\x1b[?2048h", 24), b"");
        assert_eq!(run(&mut f, b"\x1b[?2048l", 24), b"");
        // Stripped from compound lists, keeping the other modes.
        assert_eq!(
            run(&mut f, b"\x1b[?1049;2048h", 24),
            b"\x1b[?1049h\x1b[1;24r"
        );
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
    fn soft_reset_reasserts_scroll_region() {
        let mut f = PassthroughFilter::new();
        // DECSTR clears the scroll margins on the real terminal.
        assert_eq!(run(&mut f, b"\x1b[!p", 24), b"\x1b[!p\x1b[1;24r");
    }

    #[test]
    fn full_reset_reasserts_scroll_region() {
        let mut f = PassthroughFilter::new();
        // RIS clears the scroll margins on the real terminal.
        assert_eq!(run(&mut f, b"\x1bc", 24), b"\x1bc\x1b[1;24r");
    }

    #[test]
    fn sgr_and_cursor_moves_pass_through() {
        let mut f = PassthroughFilter::new();
        let input = b"\x1b[1;31m\x1b[10;5HX\x1b[0m";
        assert_eq!(run(&mut f, input, 24), input);
    }

    #[test]
    fn c0_inside_csi_is_executed_and_sequence_continues() {
        let mut f = PassthroughFilter::new();
        // Per the VT500 parser, a C0 control inside a CSI is executed while
        // the sequence continues collecting — the filter must not desync from
        // the terminal and treat the remainder as plain text.
        assert_eq!(run(&mut f, b"\x1b[1\rH", 24), b"\r\x1b[1H");
        // A CSI final smuggled past a C0 still gets rewritten.
        assert_eq!(run(&mut f, b"\x1b[\rr", 24), b"\r\x1b[1;24r");
    }

    #[test]
    fn can_aborts_csi() {
        let mut f = PassthroughFilter::new();
        // CAN drops the partial sequence; following bytes are plain text.
        assert_eq!(run(&mut f, b"\x1b[12\x18X", 24), b"\x18X");
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
    fn osc_split_across_reads_is_held_until_complete() {
        let mut f = PassthroughFilter::new();
        let mut out = Vec::new();
        // The output must never end mid-OSC: host writes between reads would
        // be injected into the string on the real terminal.
        f.rewrite(b"\x1b]52;c;aGVsbG8", 24, &mut out);
        assert_eq!(out, b"", "partial OSC must be withheld");
        f.rewrite(b"\x07after", 24, &mut out);
        assert_eq!(out, b"\x1b]52;c;aGVsbG8\x07after");
    }

    #[test]
    fn dcs_payload_passes_through() {
        let mut f = PassthroughFilter::new();
        let input = b"\x1bP1$r0m\x1b\\after";
        assert_eq!(run(&mut f, input, 24), input);
    }

    #[test]
    fn dcs_split_across_reads_is_held_until_complete() {
        let mut f = PassthroughFilter::new();
        let mut out = Vec::new();
        f.rewrite(b"\x1bP1$r0", 24, &mut out);
        assert_eq!(out, b"", "partial DCS must be withheld");
        f.rewrite(b"m\x1b\\", 24, &mut out);
        assert_eq!(out, b"\x1bP1$r0m\x1b\\");
    }

    #[test]
    fn utf8_split_across_reads_is_held_until_complete() {
        let mut f = PassthroughFilter::new();
        let mut out = Vec::new();
        // "中" (e4 b8 ad) split across reads: emitting the partial bytes would
        // let host writes interleave mid-codepoint and render U+FFFD.
        f.rewrite(b"a\xe4\xb8", 24, &mut out);
        assert_eq!(out, b"a", "partial codepoint must be withheld");
        f.rewrite(b"\xadb", 24, &mut out);
        assert_eq!(out, "a中b".as_bytes());
    }

    #[test]
    fn invalid_utf8_is_flushed_verbatim() {
        let mut f = PassthroughFilter::new();
        // A lead byte followed by a non-continuation must not swallow bytes.
        assert_eq!(run(&mut f, b"\xe4Xy", 24), b"\xe4Xy");
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
}
