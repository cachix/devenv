#!/usr/bin/env python3
"""Generate a complete sitemap of the devenv docs, including heading anchors.

MkDocs already emits a ``sitemap.xml`` listing every page, but it does not
include the ``#`` heading anchors within each page. This script crawls a
running ``mkdocs serve`` (or ``mkdocs build`` output served statically),
reads its ``sitemap.xml`` for the page list, then fetches each page and
extracts every heading anchor to produce a *complete* sitemap.

Only the Python standard library is used, so it runs anywhere.

Usage (with ``devenv up`` serving the docs):

    python3 gen/sitemap_with_anchors.py                 # text tree to stdout
    python3 gen/sitemap_with_anchors.py --format xml    # extended sitemap.xml
    python3 gen/sitemap_with_anchors.py --format json   # machine-readable
    python3 gen/sitemap_with_anchors.py --base-url http://127.0.0.1:8000 \
        --format xml --output sitemap-full.xml
"""

from __future__ import annotations

import argparse
import json
import sys
from html.parser import HTMLParser
from urllib.error import URLError
from urllib.parse import urljoin, urlparse
from urllib.request import Request, urlopen
from xml.etree import ElementTree

SITEMAP_NS = "http://www.sitemaps.org/schemas/sitemap/0.9"
HEADING_TAGS = {"h1", "h2", "h3", "h4", "h5", "h6"}


class HeadingParser(HTMLParser):
    """Collect ``(id, level, text)`` for every heading with an ``id``.

    Material for MkDocs renders headings as ``<h2 id="slug">Title<a ...>``,
    so the anchor is the heading's own ``id`` attribute. The trailing
    permalink ``<a class="headerlink">`` is ignored via ``_depth`` tracking.
    """

    def __init__(self) -> None:
        super().__init__()
        self.headings: list[dict] = []
        self._current: dict | None = None
        self._depth = 0

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag in HEADING_TAGS:
            attr = dict(attrs)
            hid = attr.get("id")
            if hid:
                self._current = {"id": hid, "level": int(tag[1]), "text": ""}
                self._depth = 1
                return
        if self._current is not None:
            # Skip text inside the permalink anchor appended to the heading.
            if tag == "a" and "headerlink" in (dict(attrs).get("class") or ""):
                self._depth = -1  # sentinel: suppress until this <a> closes
            elif self._depth > 0:
                self._depth += 1

    def handle_endtag(self, tag: str) -> None:
        if self._current is None:
            return
        if self._depth == -1 and tag == "a":
            self._depth = 1
            return
        if tag in HEADING_TAGS and self._depth != -1:
            self._current["text"] = " ".join(self._current["text"].split())
            self.headings.append(self._current)
            self._current = None
            self._depth = 0
        elif self._depth > 0:
            self._depth -= 1

    def handle_data(self, data: str) -> None:
        if self._current is not None and self._depth > 0:
            self._current["text"] += data


def fetch(url: str) -> bytes:
    req = Request(url, headers={"User-Agent": "sitemap-with-anchors/1.0"})
    with urlopen(req, timeout=30) as resp:
        return resp.read()


def page_urls_from_sitemap(base_url: str) -> list[str]:
    sitemap_url = urljoin(base_url + "/", "sitemap.xml")
    root = ElementTree.fromstring(fetch(sitemap_url))
    urls = [loc.text.strip() for loc in root.iter(f"{{{SITEMAP_NS}}}loc") if loc.text]
    if not urls:
        raise SystemExit(f"no <loc> entries found in {sitemap_url}")
    return urls


def normalize(base_url: str, page_url: str) -> str:
    """Rewrite an absolute sitemap URL onto the requested base host/port."""
    base = urlparse(base_url)
    page = urlparse(page_url)
    return page._replace(scheme=base.scheme, netloc=base.netloc).geturl()


def anchors_for_page(url: str) -> list[dict]:
    parser = HeadingParser()
    parser.feed(fetch(url).decode("utf-8", errors="replace"))
    return parser.headings


def build(base_url: str, public_url: str) -> list[dict]:
    """Crawl ``base_url`` (the live server) but emit ``public_url`` links.

    Pages are fetched from the local server, while the URLs recorded in the
    output are re-hosted onto ``public_url`` (e.g. https://devenv.sh) so the
    sitemap matches production.
    """
    base_url = base_url.rstrip("/")
    public_url = public_url.rstrip("/")
    pages = []
    for raw in page_urls_from_sitemap(base_url):
        fetch_url = normalize(base_url, raw)
        public = normalize(public_url, raw)
        try:
            headings = anchors_for_page(fetch_url)
        except URLError as exc:
            print(f"warning: failed to fetch {fetch_url}: {exc}", file=sys.stderr)
            headings = []
        pages.append({"url": public, "headings": headings})
    return pages


def render_text(pages: list[dict]) -> str:
    lines = []
    for page in pages:
        lines.append(page["url"])
        for h in page["headings"]:
            indent = "  " * h["level"]
            lines.append(f"{indent}{page['url']}#{h['id']}  ({h['text']})")
    return "\n".join(lines) + "\n"


def render_json(pages: list[dict]) -> str:
    return json.dumps(pages, indent=2, ensure_ascii=False) + "\n"


def render_xml(pages: list[dict]) -> str:
    urlset = ElementTree.Element("urlset", xmlns=SITEMAP_NS)
    for page in pages:
        _url(urlset, page["url"])
        for h in page["headings"]:
            _url(urlset, f"{page['url']}#{h['id']}")
    ElementTree.indent(urlset, space="  ")
    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        + ElementTree.tostring(urlset, encoding="unicode")
        + "\n"
    )


def _url(urlset: ElementTree.Element, loc: str) -> None:
    url = ElementTree.SubElement(urlset, "url")
    ElementTree.SubElement(url, "loc").text = loc


RENDERERS = {"text": render_text, "json": render_json, "xml": render_xml}


def site_url_from_mkdocs(default: str = "https://devenv.sh") -> str:
    """Read ``site_url`` from ``mkdocs.yml`` next to this script's docs root.

    Parsed line-by-line to avoid a PyYAML dependency; falls back to
    ``default`` if the file or key is missing.
    """
    from pathlib import Path

    config = Path(__file__).resolve().parent.parent / "mkdocs.yml"
    try:
        for line in config.read_text(encoding="utf-8").splitlines():
            if line.startswith("site_url:"):
                return line.split(":", 1)[1].strip().strip("\"'")
    except OSError:
        pass
    return default


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--base-url",
        default="http://127.0.0.1:8000",
        help="root URL of the running docs server to crawl (default: %(default)s)",
    )
    ap.add_argument(
        "--public-url",
        default=None,
        help="host to re-host the emitted links onto so they match production "
        "(default: site_url from mkdocs.yml, else https://devenv.sh)",
    )
    ap.add_argument("--format", choices=RENDERERS, default="text")
    ap.add_argument("--output", "-o", help="write to a file instead of stdout")
    args = ap.parse_args()

    public_url = args.public_url or site_url_from_mkdocs()
    pages = build(args.base_url, public_url)
    out = RENDERERS[args.format](pages)

    total_anchors = sum(len(p["headings"]) for p in pages)
    print(
        f"discovered {len(pages)} pages, {total_anchors} heading anchors",
        file=sys.stderr,
    )

    if args.output:
        with open(args.output, "w", encoding="utf-8") as fh:
            fh.write(out)
        print(f"wrote {args.output}", file=sys.stderr)
    else:
        try:
            sys.stdout.write(out)
        except BrokenPipeError:
            # Downstream consumer (e.g. `head`) closed the pipe early.
            sys.stdout = None


if __name__ == "__main__":
    main()
