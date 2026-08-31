from __future__ import annotations

import argparse
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import urlsplit


class SiteParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.references: list[str] = []
        self.has_description = False
        self.has_viewport = False
        self.has_title = False
        self.script_count = 0

    def handle_starttag(
        self, tag: str, attrs: list[tuple[str, str | None]]
    ) -> None:
        attributes = dict(attrs)
        if tag == "title":
            self.has_title = True
        if tag == "script":
            self.script_count += 1
        if tag == "meta" and attributes.get("name") == "description":
            self.has_description = bool(attributes.get("content"))
        if tag == "meta" and attributes.get("name") == "viewport":
            self.has_viewport = bool(attributes.get("content"))
        for name in ("href", "src"):
            if attributes.get(name):
                self.references.append(attributes[name] or "")


def validate_site(site: Path) -> None:
    index = site / "index.html"
    if not index.is_file():
        raise ValueError(f"Missing Pages entry point: {index}")

    parser = SiteParser()
    parser.feed(index.read_text(encoding="utf-8"))
    if not (parser.has_title and parser.has_description and parser.has_viewport):
        raise ValueError("Pages entry point is missing required metadata")
    if parser.script_count:
        raise ValueError("The guide landing page must not require client-side scripts")

    failures: list[str] = []
    for reference in parser.references:
        parts = urlsplit(reference)
        if parts.scheme or parts.netloc or reference.startswith(("#", "mailto:")):
            continue
        target = parts.path or "index.html"
        if target.endswith("/"):
            target = f"{target}index.html"
        resolved = (site / target).resolve()
        if site.resolve() not in resolved.parents and resolved != site.resolve():
            failures.append(f"Reference escapes the Pages artifact: {reference}")
        elif not resolved.is_file():
            failures.append(f"Missing Pages file: {reference}")
    if failures:
        raise ValueError("Invalid Pages references:\n" + "\n".join(failures))

    print(f"Verified GitHub Pages content in {site}")


def main() -> None:
    parser = argparse.ArgumentParser(description="Validate the guide Pages artifact.")
    parser.add_argument("site", type=Path)
    validate_site(parser.parse_args().site.resolve())


if __name__ == "__main__":
    main()
