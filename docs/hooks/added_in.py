"""Expand the `[added-in:X.Y]` shorthand into the small "added in" version marker.

Write `[added-in:1.2]` on its own line directly after a heading (or inline) instead
of the full `<small class="added-in">Added in `1.2`</small>` HTML. On reference
pages, markers following section headings are moved inside the heading after
Markdown rendering so the heading keeps its normal margins. Styling lives in
`assets/top-level.css`.

Use this for stable, long-standing features. Reserve the prominent
`!!! tip "New in version X"` admonition for recent (2.0+) additions.
"""

import re

# [added-in:1.2] / [added-in: 2.0.5] -> version is a dotted number.
ADDED_IN_RE = re.compile(r"\[added-in:\s*([0-9]+(?:\.[0-9]+)*)\]")

FENCE_RE = re.compile(r"^\s*(```|~~~)")

# Markdown renders a marker on its own line as a paragraph. On reference pages,
# move markers that immediately follow h2/h3 headings into the heading after the
# TOC and heading IDs have been generated. This keeps reference markers inline
# without changing the heading's layout or anchor.
SECTION_MARKER_RE = re.compile(
    r"(?P<heading><h(?P<level>[23])\b[^\n]*?)(?P<closing></h(?P=level)>)\s*"
    r'<p>(?P<marker><small class="added-in">[^\n]*?</small>)</p>',
)


def _marker(match):
    version = match.group(1)
    return f'<small class="added-in">Added in `{version}`</small>'


def on_page_markdown(markdown, **kwargs):
    # Substitute outside fenced code blocks so examples stay literal.
    out = []
    in_fence = False
    for line in markdown.split("\n"):
        if FENCE_RE.match(line):
            in_fence = not in_fence
        if not in_fence:
            line = ADDED_IN_RE.sub(_marker, line)
        out.append(line)
    return "\n".join(out)


def on_page_content(html, page, **kwargs):
    if not page.file.src_uri.startswith("reference/"):
        return html

    def move_marker(match):
        return (
            f"{match.group('heading')} {match.group('marker')}"
            f"{match.group('closing')}"
        )

    return SECTION_MARKER_RE.sub(move_marker, html)
