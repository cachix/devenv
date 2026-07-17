#!/usr/bin/env python3
"""Diff two docs sitemaps (old vs new) and propose ``_redirects`` entries.

Feeds off the maps produced by ``sitemap_with_anchors.py``. Each side may be
either a JSON map file produced by that script, or a live base URL (e.g.
``http://127.0.0.1:8000``) which will be crawled on the fly.

The output is **best effort and advisory** — a human must review the proposed
redirects before they land in ``_redirects``. The diff always reports which
links were *added* and *removed*; for removed links it additionally tries to
guess a *moved* target (by anchor slug or heading text) so most redirects come
pre-filled.

Usage:

    # snapshot the old site once (e.g. from an archived deploy)
    python3 gen/sitemap_with_anchors.py --base-url http://127.0.0.1:9000 \
        --format json -o old.json

    # snapshot the current site
    python3 gen/sitemap_with_anchors.py --format json -o new.json

    # diff them -> proposed redirects on stdout, report on stderr
    python3 gen/sitemap_diff.py old.json new.json

    # or crawl both sides live, and skip anything already redirected
    python3 gen/sitemap_diff.py http://127.0.0.1:9000 http://127.0.0.1:8000 \
        --existing _redirects -o proposed-redirects.txt
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from urllib.parse import urlparse

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from sitemap_with_anchors import build  # noqa: E402


def normalize_text(text: str) -> str:
    return " ".join(text.split()).casefold()


def path_of(url: str) -> str:
    """Relative path used in ``_redirects`` (host stripped)."""
    p = urlparse(url)
    return p.path or "/"


class SiteMap:
    """A site's link inventory, keyed on host-independent paths.

    ``pages`` maps a page path -> its h1 heading text (for title matching).
    ``anchors`` maps ``path#slug`` -> ``{"slug", "text", "page"}``.
    """

    def __init__(self, pages_json: list[dict]) -> None:
        self.pages: dict[str, str] = {}
        self.anchors: dict[str, dict] = {}
        # page path -> anchor keys in document order (for same-page pairing).
        self.page_anchors: dict[str, list[str]] = {}
        for page in pages_json:
            page_path = path_of(page["url"])
            h1 = next((h for h in page["headings"] if h["level"] == 1), None)
            self.pages[page_path] = h1["text"] if h1 else ""
            order = self.page_anchors.setdefault(page_path, [])
            for h in page["headings"]:
                key = f"{page_path}#{h['id']}"
                self.anchors[key] = {
                    "slug": h["id"],
                    "text": h["text"],
                    "level": h["level"],
                    "page": page_path,
                }
                order.append(key)

    def keys(self) -> set[str]:
        return set(self.pages) | set(self.anchors)


class NewIndex:
    """Reverse indexes over the new map, for best-effort move detection."""

    def __init__(self, new: SiteMap) -> None:
        self.pages = new.pages
        self.anchors = new.anchors
        self.by_slug: dict[str, list[str]] = {}
        self.by_anchor_text: dict[str, list[str]] = {}
        self.by_h1: dict[str, list[str]] = {}
        self.by_basename: dict[str, list[str]] = {}
        for key, a in new.anchors.items():
            self.by_slug.setdefault(a["slug"], []).append(key)
            self.by_anchor_text.setdefault(normalize_text(a["text"]), []).append(key)
        for path, h1 in new.pages.items():
            if h1:
                self.by_h1.setdefault(normalize_text(h1), []).append(path)
            self.by_basename.setdefault(_basename(path), []).append(path)


def _basename(path: str) -> str:
    return path.rstrip("/").rsplit("/", 1)[-1]


def _unique(index: dict[str, list[str]], key: str) -> str | None:
    hits = index.get(key)
    return hits[0] if hits and len(hits) == 1 else None


def match_anchor(entry: dict, new: NewIndex) -> tuple[str, int, str] | None:
    """Guess where a removed anchor moved to. Returns (target, status, reason)."""
    # A heading keeps its slug when it moves pages -> strongest signal.
    hit = _unique(new.by_slug, entry["slug"])
    if hit:
        return hit, 301, "slug match"
    # Heading was re-slugged but the title is unchanged and unique.
    hit = _unique(new.by_anchor_text, normalize_text(entry["text"]))
    if hit:
        return hit, 301, "heading text match"
    # The whole page moved (same basename elsewhere); keep the fragment if it
    # survived, else fall back to the relocated page.
    page = _unique(new.by_basename, _basename(entry["page"]))
    if page and page != entry["page"]:
        anchored = f"{page}#{entry['slug']}"
        if anchored in new.anchors:
            return anchored, 302, "page moved (slug preserved)"
        return page, 302, "page moved (fragment dropped)"
    return None


def parse_existing(path: str | None) -> tuple[set[str], list[str]]:
    """Return (exact source paths, splat prefixes) already covered."""
    exact: set[str] = set()
    splats: list[str] = []
    if not path or not os.path.exists(path):
        return exact, splats
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            source = line.split()[0]
            if source.endswith("*"):
                splats.append(source[:-1])
            else:
                exact.add(source)
    return exact, splats


def is_covered(path: str, exact: set[str], splats: list[str]) -> bool:
    return path in exact or any(path.startswith(prefix) for prefix in splats)


def load_map(source: str) -> SiteMap:
    if source.startswith(("http://", "https://")):
        return SiteMap(build(source, source))
    with open(source, encoding="utf-8") as fh:
        return SiteMap(json.load(fh))


def diff(old: SiteMap, new: SiteMap) -> dict:
    new_index = NewIndex(new)
    removed_set = old.keys() - new.keys()
    added_set = new.keys() - old.keys()
    removed = sorted(removed_set)
    added = sorted(added_set)

    same_page = same_page_pairs(old, new, removed_set, added_set)

    proposals = []
    for key in removed:
        if "#" in key:
            match = match_anchor(old.anchors[key], new_index) or same_page.get(key)
        else:
            # Feed the old page's h1 text through the title index by hand,
            # since match_page only sees paths.
            match = _match_removed_page(key, old, new_index)
        proposals.append({"source": key, "match": match})
    return {"added": added, "removed": removed, "proposals": proposals}


def same_page_pairs(
    old: SiteMap, new: SiteMap, removed: set[str], added: set[str]
) -> dict[str, tuple[str, int, str]]:
    """Pair renamed headings within a page that still exists.

    When both a heading's slug and its text change (e.g. "Creating files" ->
    "Declarative files") no direct signal survives, but if the page is still
    present we can pair its removed and added anchors by heading level in
    document order. This is a heuristic — always emitted as 302 for review.
    """
    mapping: dict[str, tuple[str, int, str]] = {}
    for page in set(old.pages) & set(new.pages):
        rem = [k for k in old.page_anchors.get(page, []) if k in removed]
        add = [k for k in new.page_anchors.get(page, []) if k in added]
        if not rem or not add:
            continue
        add_by_level: dict[int, list[str]] = {}
        for k in add:
            add_by_level.setdefault(new.anchors[k]["level"], []).append(k)
        used: dict[int, int] = {}
        for rk in rem:
            level = old.anchors[rk]["level"]
            candidates = add_by_level.get(level, [])
            i = used.get(level, 0)
            if i < len(candidates):
                mapping[rk] = (candidates[i], 301, "same-page guess")
                used[level] = i + 1
    return mapping


def _match_removed_page(
    page_path: str, old: SiteMap, new: NewIndex
) -> tuple[str, int, str] | None:
    h1 = old.pages.get(page_path, "")
    if h1:
        hit = _unique(new.by_h1, normalize_text(h1))
        if hit:
            return hit, 301, "title match"
    hit = _unique(new.by_basename, _basename(page_path))
    if hit and hit != page_path:
        return hit, 302, "basename match"
    return None


def render(result: dict, existing: tuple[set[str], list[str]]) -> tuple[str, str]:
    """Return (redirects_stdout, report_stderr)."""
    exact, splats = existing
    added, removed = result["added"], result["removed"]

    matched = [p for p in result["proposals"] if p["match"]]
    unmatched = [p for p in result["proposals"] if not p["match"]]

    out = ["# Proposed redirects — REVIEW before committing to _redirects.",
           f"# {len(removed)} removed, {len(added)} added, "
           f"{len(matched)} auto-matched, {len(unmatched)} need a target.",
           ""]
    for p in result["proposals"]:
        if is_covered(p["source"], exact, splats):
            continue
        if p["match"]:
            target, status, reason = p["match"]
            out.append(f"{p['source']} {target} {status}  # {reason}")
        else:
            out.append(f"# TODO pick target: {p['source']} <?> 302")

    report = [
        "=== sitemap diff ===",
        f"removed: {len(removed)}   added: {len(added)}",
        "",
        "--- REMOVED (need redirects) ---",
        *(f"  - {k}" for k in removed),
        "",
        "--- ADDED (informational) ---",
        *(f"  + {k}" for k in added),
    ]
    return "\n".join(out) + "\n", "\n".join(report) + "\n"


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("old", help="old site: JSON map file or base URL to crawl")
    ap.add_argument("new", help="new site: JSON map file or base URL to crawl")
    ap.add_argument(
        "--existing",
        help="path to an existing _redirects to skip already-covered sources",
    )
    ap.add_argument(
        "--output", "-o", help="write proposed redirects here instead of stdout"
    )
    ap.add_argument(
        "--json", action="store_true", help="emit the raw diff as JSON to stdout"
    )
    args = ap.parse_args()

    old = load_map(args.old)
    new = load_map(args.new)
    result = diff(old, new)

    if args.json:
        sys.stdout.write(json.dumps(result, indent=2, ensure_ascii=False) + "\n")
        return

    redirects, report = render(result, parse_existing(args.existing))
    sys.stderr.write(report)
    if args.output:
        with open(args.output, "w", encoding="utf-8") as fh:
            fh.write(redirects)
        print(f"\nwrote {args.output}", file=sys.stderr)
    else:
        sys.stdout.write(redirects)


if __name__ == "__main__":
    main()
