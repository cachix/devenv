//! Helpers for shaping Nix evaluation errors into miette diagnostics.

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
