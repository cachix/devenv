"""
MkDocs hook for processing shortcode comments into styled badges.

Usage in markdown:
    <!-- md:flag cli-feature -->

Renders as a styled badge with icon and text.
"""

import re
from mkdocs.structure.pages import Page


def on_page_markdown(markdown: str, page: Page, **kwargs) -> str:
    """Process shortcode comments in markdown content."""

    def replace(match: re.Match) -> str:
        shortcode_type = match.group(1)
        args = match.group(2).strip()

        if shortcode_type == "flag":
            return _badge_for_flag(args)

        return match.group(0)

    return re.sub(
        r"<!-- md:(\w+)(.*?) -->", replace, markdown, flags=re.IGNORECASE | re.MULTILINE
    )


def _badge(icon: str, text: str, badge_type: str = "", tooltip: str = "") -> str:
    """Generate HTML for a badge with icon and text."""
    classes = "md-badge"
    if badge_type:
        classes += f" md-badge--{badge_type}"

    title_attr = f' title="{tooltip}"' if tooltip else ""

    return "".join(
        [
            f'<span class="{classes}"{title_attr}>',
            f'<span class="md-badge__icon">{icon}</span>',
            f'<span class="md-badge__text">{text}</span>',
            "</span>",
        ]
    )


def _badge_for_flag(flag: str) -> str:
    """Generate badge for feature flags like 'cli-feature'."""
    flags = {
        "cli-feature": {
            "icon": ":material-creation-outline:",
            "text": "devenv CLI",
            "type": "cli",
            "tooltip": "This feature is exclusive to the devenv CLI",
        },
    }

    if flag in flags:
        config = flags[flag]
        return _badge(
            config["icon"],
            config["text"],
            config.get("type", ""),
            config.get("tooltip", ""),
        )

    return _badge("", flag, "")
