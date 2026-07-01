//! Helpers for shaping Nix evaluation errors into miette diagnostics.
//!
//! These work on the Nix C bindings' already-rendered error text. There is no
//! structured error type exposed by `nix-bindings-rust` to key off; the
//! bindings stringify the C++ exception's `what()` into a single message and
//! return it as `anyhow::Error`.
//!
//! The eval-returned error is always rendered in full and in Nix's natural
//! order: `--show-trace` frames first, the actionable `error: …` paragraph
//! last. That keeps the actionable message at the bottom of the terminal
//! where the cursor lands, instead of requiring the user to scroll up past
//! the trace to find it.

/// Shape a raw Nix error string into a `MietteDiagnostic` for rendering.
///
/// Pure function over the FFI return text so it can be unit-tested without
/// running an actual evaluation. The caller wraps the result with
/// `Report::from(..)` to get a `miette::Error`.
///
/// The raw text is rendered as-is (modulo dedenting) — no reordering, no
/// filtering — so whatever eval returned is shown in full.
pub(crate) fn format_eval_error(raw: &str, context: &str) -> miette::MietteDiagnostic {
    miette::diagnostic!("{context}: {}", dedent_lines(raw))
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
    fn format_eval_error_keeps_error_below_trace() {
        // Missing-input shape: Nix `--show-trace` frames followed by an
        // assertion paragraph. Natural order is preserved — frames first,
        // the actionable error last — so the suggestion sits at the bottom
        // of the terminal output.
        let raw = "\
… from call site
  at /tmp/devenv.nix:1:1:
… while calling 'throw' builtin

error: Failed assertions:
- To use 'git-hooks', run the following command:

    $ devenv inputs add git-hooks github:cachix/git-hooks.nix --follows nixpkgs";
        let diag = format_eval_error(raw, "Failed to get shell attribute from devenv");
        assert!(
            diag.message
                .starts_with("Failed to get shell attribute from devenv: "),
            "message should be prefixed with the context, got: {}",
            diag.message
        );
        let trace_pos = diag
            .message
            .find("… from call site")
            .expect("trace frames should be in the message");
        let suggestion_pos = diag
            .message
            .find("devenv inputs add git-hooks")
            .expect("the actionable suggestion should be in the message");
        assert!(
            trace_pos < suggestion_pos,
            "the trace must precede the error so the actionable part is last:\n{}",
            diag.message
        );
        assert!(
            diag.message.trim_end().ends_with("--follows nixpkgs"),
            "the actionable suggestion should be the last thing rendered:\n{}",
            diag.message
        );
    }

    #[test]
    fn format_eval_error_renders_bare_syntax_error() {
        // Syntax-error shape: a single `error: …` paragraph with source
        // context and no frames.
        let raw =
            "error: syntax error, unexpected '}', expecting ';'\n       at /tmp/devenv.nix:5:1:";
        let diag = format_eval_error(raw, "Failed to get attribute 'config.cachix.enable'");
        assert!(diag.message.contains("syntax error"));
        assert!(diag.message.contains("devenv.nix"));
    }

    #[test]
    fn format_eval_error_always_includes_the_eval_error() {
        // Regression lock: a stale Nix `warning: …` line must never replace
        // the eval-returned error. The raw FFI text is rendered in full, so
        // even if a warning is present the real error follows it at the bottom.
        let raw = "\
warning: Ignoring the client-specified setting 'system', because it is a restricted setting and you are not a trusted user

error: syntax error, unexpected '}', expecting ';'
       at /tmp/devenv.nix:5:1:";
        let diag = format_eval_error(raw, "Failed to get shell attribute from devenv");
        let warning_pos = diag
            .message
            .find("Ignoring the client-specified setting")
            .expect("the warning text is part of the raw error and stays visible");
        let error_pos = diag
            .message
            .find("error: syntax error")
            .expect("the eval error must always be rendered");
        assert!(
            warning_pos < error_pos,
            "the eval error must come after (below) the warning:\n{}",
            diag.message
        );
    }

    #[test]
    fn format_eval_error_renders_input_with_no_error_line() {
        // Degraded shape — if Nix ever returns text without an `error:`
        // line (format change, unrelated diagnostic, etc.), the whole text
        // is still shown.
        let raw = "some unexpected text\nthat does not contain the keyword";
        let diag = format_eval_error(raw, "Failed to evaluate");
        assert!(diag.message.contains("some unexpected text"));
        assert!(diag.message.contains("does not contain the keyword"));
    }

    #[test]
    fn format_eval_error_strips_common_indentation() {
        // Nix's C-bindings logger wraps trace output in a uniform left margin
        // (7 spaces in practice). `dedent_lines` strips that so the message
        // reads at the natural depth instead of being shoved off-screen.
        let raw = "\
       … from call site
         at /tmp/devenv.nix:1:1:

       error: boom";
        let diag = format_eval_error(raw, "ctx");
        // The first raw line lands on the context line; the rest of the
        // margin is stripped so the `at …` pointer keeps only its relative
        // two-space depth and the error paragraph starts at column 0.
        assert!(diag.message.starts_with("ctx: … from call site"));
        assert!(
            diag.message.contains("\n  at /tmp/devenv.nix:1:1:"),
            "location line should keep only its relative depth, message was:\n{}",
            diag.message
        );
        assert!(diag.message.ends_with("\nerror: boom"));
    }

    #[test]
    fn dedent_lines_preserves_relative_depth() {
        let text = "       … frame\n         at /tmp/x.nix:1:1:\n\n       error: boom";
        let dedented = dedent_lines(text);
        assert!(dedented.lines().next().unwrap().starts_with("… frame"));
        assert!(dedented.contains("\n  at /tmp/x.nix:1:1:"));
        assert!(dedented.ends_with("error: boom"));
    }
}
