//! Helpers for shaping Nix evaluation errors into miette diagnostics.

/// Returns true if the message looks like a Nix warning (e.g. starts with
/// `warning:`). Some Nix warnings (such as restricted-settings notices) get
/// emitted through the FFI logger at the error verbosity, which would otherwise
/// shadow the actual evaluation error we want to surface.
pub(crate) fn looks_like_warning(msg: &str) -> bool {
    let trimmed = msg.trim_start();
    trimmed.starts_with("warning:") || trimmed.starts_with("\u{1b}[33;1mwarning:")
}

/// Pick the most useful raw error text to display.
///
/// Prefer the most recent entry from `nix_errors` that is not a warning —
/// when Nix logs a full tree-style trace through the FFI logger, we want to
/// surface that. If no real error was logged (e.g. a syntax error where the
/// diagnostic only arrives via the FFI return value), fall back to
/// `ffi_fallback`.
pub(crate) fn select_raw_error<'a>(
    nix_errors: &'a [String],
    ffi_fallback: impl FnOnce() -> String,
) -> std::borrow::Cow<'a, str> {
    nix_errors
        .iter()
        .rev()
        .find(|msg| !looks_like_warning(msg))
        .map(|s| std::borrow::Cow::Borrowed(s.as_str()))
        .unwrap_or_else(|| std::borrow::Cow::Owned(ffi_fallback()))
}

/// Strip the longest leading whitespace common to non-blank, non-zero-indent
/// lines. Nix's `--show-trace` indents each trace line; removing the common
/// prefix flattens the rendered diagnostic without altering relative depth
/// (so source-pointer carets stay aligned).
pub(crate) fn dedent_lines(text: &str) -> String {
    let min_indent = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.bytes().take_while(|b| *b == b' ').count())
        .filter(|n| *n > 0)
        .min()
        .unwrap_or(0);
    if min_indent == 0 {
        return text.to_string();
    }
    text.lines()
        .map(|l| {
            let strip = l.bytes().take_while(|b| *b == b' ').count().min(min_indent);
            &l[strip..]
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warning_prefix_is_detected() {
        assert!(looks_like_warning(
            "warning: Ignoring the client-specified setting 'system'"
        ));
        assert!(looks_like_warning(
            "  warning: leading whitespace is tolerated"
        ));
        assert!(looks_like_warning("\u{1b}[33;1mwarning:\u{1b}[0m colored"));
    }

    #[test]
    fn error_prefix_is_not_treated_as_warning() {
        assert!(!looks_like_warning(
            "error: syntax error, unexpected '}', expecting ';'"
        ));
        assert!(!looks_like_warning("\u{1b}[31;1merror:\u{1b}[0m foo"));
        assert!(!looks_like_warning(""));
    }

    #[test]
    fn select_raw_error_falls_back_to_ffi_when_only_warnings_logged() {
        // Reproduces issue #2820: a stale Nix warning shouldn't shadow the
        // real FFI error (a syntax error in devenv.nix in that scenario).
        let nix_errors = vec![
            "warning: Ignoring the client-specified setting 'system', because \
             it is a restricted setting and you are not a trusted user"
                .to_string(),
        ];
        let ffi = || "error: syntax error, unexpected '}', expecting ';'".to_string();
        assert_eq!(
            select_raw_error(&nix_errors, ffi),
            "error: syntax error, unexpected '}', expecting ';'"
        );
    }

    #[test]
    fn select_raw_error_prefers_logged_error_over_ffi() {
        let nix_errors = vec![
            "warning: stale warning from earlier".to_string(),
            "error: real error logged via the FFI callback".to_string(),
        ];
        let ffi = || "ffi-fallback".to_string();
        assert_eq!(
            select_raw_error(&nix_errors, ffi),
            "error: real error logged via the FFI callback"
        );
    }

    #[test]
    fn select_raw_error_skips_trailing_warning_to_find_earlier_error() {
        let nix_errors = vec![
            "error: real error logged via the FFI callback".to_string(),
            "warning: noisy warning emitted after the error".to_string(),
        ];
        let ffi = || "ffi-fallback".to_string();
        assert_eq!(
            select_raw_error(&nix_errors, ffi),
            "error: real error logged via the FFI callback"
        );
    }

    #[test]
    fn select_raw_error_uses_ffi_when_no_logs() {
        let ffi = || "ffi-only".to_string();
        assert_eq!(select_raw_error(&[], ffi), "ffi-only");
    }
}
