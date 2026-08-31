#!/usr/bin/env python3
"""Validate all static pages, canonical metadata, local assets and navigation."""
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2] / "site" / "out"
ORIGIN = "https://antonillos.github.io"
BASE = "/safeselect"
ROUTES = ["/", "/compare/", "/guides/dbeaver-codex/", "/read-only-is-not-a-boolean/"]
GOOGLE_VERIFICATION_FILE = "googled7be89f4207cbfe7.html"
GOOGLE_VERIFICATION_CONTENT = b"google-site-verification: googled7be89f4207cbfe7.html"


class Page(HTMLParser):
    def __init__(self, text):
        super().__init__()
        self.ids, self.links, self.meta = set(), [], {}
        self.canonical = None
        self.h1 = self.scripts = 0
        self.in_title = False
        self.title = ""
        self.feed(text)

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if "id" in attrs:
            if attrs["id"] in self.ids:
                raise ValueError(f"Duplicate id: {attrs['id']}")
            self.ids.add(attrs["id"])
        if tag == "h1":
            self.h1 += 1
        if tag == "script":
            self.scripts += 1
        if tag == "title":
            self.in_title = True
        if tag == "meta":
            self.meta[attrs.get("name", attrs.get("property"))] = attrs.get("content")
        if tag == "link" and attrs.get("rel") == "canonical":
            self.canonical = attrs.get("href")
        if tag in {"a", "link", "img"}:
            url = attrs.get("href", attrs.get("src"))
            if url:
                self.links.append(url)
        if tag == "img":
            # Explicitly decorative images may use null alt; missing alt is
            # still an error, even when aria-hidden is present.
            decorative = attrs.get("alt") == "" and attrs.get("aria-hidden") == "true"
            if not (attrs.get("alt") or "").strip() and not decorative:
                raise ValueError("Image missing alternative text")

    def handle_endtag(self, tag):
        if tag == "title":
            self.in_title = False

    def handle_data(self, data):
        if self.in_title:
            self.title += data


def validate(root=ROOT):
    verification = root / GOOGLE_VERIFICATION_FILE
    assert verification.is_file(), "missing Google verification file"
    assert verification.read_bytes() == GOOGLE_VERIFICATION_CONTENT, "changed Google verification content"
    pages = {route: Page((root / route.strip("/") / "index.html").read_text()) for route in ROUTES}
    for route, page in pages.items():
        assert page.h1 == 1, f"{route}: expected one h1"
        assert page.scripts == 0, f"{route}: static site must not require JavaScript or trackers"
        assert page.canonical == f"{ORIGIN}{BASE}{route}", f"{route}: wrong canonical"
        assert page.meta["og:url"] == page.canonical
        assert page.title and page.title == page.meta["og:title"] == page.meta["twitter:title"]
        assert page.meta["description"] == page.meta["og:description"] == page.meta["twitter:description"]
        assert "noindex" not in page.meta.get("robots", "")
        if route == "/":
            assert page.meta["og:image"] == f"{ORIGIN}{BASE}/og.png"
            assert page.meta["twitter:image"] == page.meta["og:image"]
        else:
            assert "og:image" not in page.meta and "twitter:image" not in page.meta
        for link in page.links:
            url = urlsplit(link)
            assert url.scheme not in {"javascript", "data"}, f"Unsafe link: {link}"
            if url.scheme or url.netloc:
                if f"{url.scheme}://{url.netloc}" != ORIGIN or not url.path.startswith(f"{BASE}/"):
                    continue
            path = unquote(url.path)
            if path:
                assert path.startswith(f"{BASE}/"), f"{route}: wrong base path {link}"
                path = path[len(BASE):]
            else:
                path = route
            target = root / path.strip("/")
            if target.is_dir():
                target /= "index.html"
            assert target.resolve().is_relative_to(root.resolve())
            assert target.is_file() and target.stat().st_size > 0, f"{route}: missing {link}"
            if url.fragment:
                assert unquote(url.fragment) in Page(target.read_text()).ids, f"{route}: missing anchor {link}"
    urls = {node.text for node in ET.parse(root / "sitemap.xml").iter("{http://www.sitemaps.org/schemas/sitemap/0.9}loc")}
    assert urls == {f"{ORIGIN}{BASE}{route}" for route in ROUTES}
    assert (root / ".nojekyll").exists()
    print(f"Validated {len(pages)} static routes, metadata, sitemap, assets and internal links.")


if __name__ == "__main__":
    validate()
