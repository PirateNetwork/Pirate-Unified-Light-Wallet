from __future__ import annotations

import argparse
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import urlsplit


class SiteParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.references: list[str] = []
        self.text: list[str] = []
        self.has_description = False
        self.has_viewport = False
        self.has_title = False
        self.has_h1 = False
        self.script_count = 0

    def handle_starttag(
        self, tag: str, attrs: list[tuple[str, str | None]]
    ) -> None:
        attributes = dict(attrs)
        if tag == "title":
            self.has_title = True
        if tag == "h1":
            self.has_h1 = True
        if tag == "script":
            self.script_count += 1
        if tag == "meta" and attributes.get("name") == "description":
            self.has_description = bool(attributes.get("content"))
        if tag == "meta" and attributes.get("name") == "viewport":
            self.has_viewport = bool(attributes.get("content"))
        for name in ("href", "src"):
            if attributes.get(name):
                self.references.append(attributes[name] or "")

    def handle_data(self, data: str) -> None:
        self.text.append(data)


def parse_page(page: Path) -> SiteParser:
    parser = SiteParser()
    parser.feed(page.read_text(encoding="utf-8"))
    return parser


def validate_page(site: Path, page: Path) -> SiteParser:
    parser = parse_page(page)
    if not (
        parser.has_title
        and parser.has_description
        and parser.has_viewport
        and parser.has_h1
    ):
        raise ValueError(f"HTML page is missing required metadata or heading: {page}")
    if parser.script_count:
        raise ValueError(f"Static Pages content must not require scripts: {page}")

    failures: list[str] = []
    for reference in parser.references:
        parts = urlsplit(reference)
        if parts.scheme or parts.netloc or reference.startswith(("#", "mailto:")):
            continue
        target = parts.path or page.name
        if target.endswith("/"):
            target = f"{target}index.html"
        resolved = (page.parent / target).resolve()
        if site not in resolved.parents and resolved != site:
            failures.append(f"Reference escapes the Pages artifact: {reference}")
        elif not resolved.is_file():
            failures.append(f"Missing Pages file: {reference}")
    if failures:
        raise ValueError(
            f"Invalid Pages references in {page.name}:\n" + "\n".join(failures)
        )

    return parser


def validate_site(site: Path) -> None:
    index = site / "index.html"
    if not index.is_file():
        raise ValueError(f"Missing Pages entry point: {index}")

    privacy_policy = site / "privacy-policy.html"
    if not privacy_policy.is_file():
        raise ValueError(f"Missing public privacy policy: {privacy_policy}")

    for page in sorted(site.rglob("*.html")):
        validate_page(site, page)

    privacy = validate_page(site, privacy_policy)
    privacy_text = " ".join(privacy.text).lower()
    required_disclosures = (
        "stashi wallet privacy policy",
        "pirate chain foundation",
        "com.pirate.wallet",
        "information sent over the network",
        "retention and deletion",
        "dev@piratechainfoundation.com",
    )
    missing_disclosures = [
        disclosure
        for disclosure in required_disclosures
        if disclosure not in privacy_text
    ]
    if missing_disclosures:
        raise ValueError(
            "Privacy policy is missing required disclosures:\n"
            + "\n".join(missing_disclosures)
        )

    print(f"Verified GitHub Pages content in {site}")


def main() -> None:
    parser = argparse.ArgumentParser(description="Validate the guide Pages artifact.")
    parser.add_argument("site", type=Path)
    validate_site(parser.parse_args().site.resolve())


if __name__ == "__main__":
    main()
