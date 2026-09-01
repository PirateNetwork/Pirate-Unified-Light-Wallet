from __future__ import annotations

import argparse
import re
from pathlib import Path

from pypdf import PdfReader
from pypdf.generic import Destination, DictionaryObject, IndirectObject


ROOT = Path(__file__).resolve().parents[2]
GUIDE = ROOT / "docs" / "user-guide"
DEFAULT_PDF = ROOT / "output" / "pdf" / "Stashi-Wallet-User-Guide.pdf"
MARKDOWN_LINK = re.compile(r"!?\[[^]]*\]\(([^)]+)\)")

REQUIRED_SECTIONS = (
    "Stashi Wallet user guide",
    "Install and set up Stashi Wallet",
    "Wallet basics",
    "Receive and send ARRR",
    "Seed accounts, keys, and addresses",
    "Move from Treasure Chest or Pirate Wallet Lite",
    "Network privacy and synchronisation",
    "Backups and wallet security",
    "Settings and release verification",
    "Troubleshooting",
    "Advanced use",
)


def slug(value: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", value.lower()).strip("-")


def markdown_anchors(path: Path) -> set[str]:
    anchors: set[str] = set()
    for line in path.read_text(encoding="utf-8").splitlines():
        match = re.match(r"^#{1,6}\s+(.+)$", line.strip())
        if match:
            anchors.add(slug(match.group(1)))
    return anchors


def validate_markdown_links() -> None:
    failures: list[str] = []
    for source in sorted(GUIDE.glob("*.md")):
        text = source.read_text(encoding="utf-8")
        for match in MARKDOWN_LINK.finditer(text):
            target = match.group(1)
            if target.startswith(("http://", "https://", "mailto:")):
                continue
            file_part, _, fragment = target.partition("#")
            target_path = (source.parent / (file_part or source.name)).resolve()
            if not target_path.is_file():
                failures.append(f"{source.relative_to(ROOT)}: missing target {target}")
                continue
            if fragment and target_path.suffix.lower() == ".md":
                if slug(fragment) not in markdown_anchors(target_path):
                    failures.append(
                        f"{source.relative_to(ROOT)}: missing heading in {target}"
                    )
    if failures:
        raise ValueError("Broken guide links:\n" + "\n".join(failures))


def validate_editorial_language() -> None:
    failures: list[str] = []
    banned_terms = {
        "phone": "Mobile",
        "app": "application",
        "color": "colour",
        "organization": "organisation",
        "finalizing": "finalising",
        "minimize": "minimise",
        "synchronization": "synchronisation",
    }
    for source in sorted(GUIDE.glob("*.md")):
        text = source.read_text(encoding="utf-8")
        visible_text = re.sub(r"\]\([^)]+\)", "]", text)
        visible_text = re.sub(r"`[^`]+`", "", visible_text)
        for term, replacement in banned_terms.items():
            if re.search(rf"\b{re.escape(term)}\b", visible_text, re.IGNORECASE):
                failures.append(
                    f"{source.relative_to(ROOT)}: use {replacement!r} instead of {term!r}"
                )
        for match in re.finditer(r"!\[([^]]*)\]\([^)]+\)", text):
            if not match.group(1).strip():
                failures.append(
                    f"{source.relative_to(ROOT)}: image is missing alternative text"
                )
    if failures:
        raise ValueError("Guide editorial checks failed:\n" + "\n".join(failures))


def outline_titles(items: list[object]) -> list[str]:
    titles: list[str] = []
    for item in items:
        if isinstance(item, list):
            titles.extend(outline_titles(item))
        elif isinstance(item, Destination):
            titles.append(item.title)
    return titles


def count_annotations(reader: PdfReader, subtype: str) -> int:
    count = 0
    for page in reader.pages:
        for annotation_ref in page.get("/Annots", []):
            annotation = annotation_ref.get_object()
            if annotation.get("/Subtype") == subtype:
                count += 1
    return count


def count_images(reader: PdfReader) -> int:
    count = 0
    for page in reader.pages:
        resources = page.get("/Resources", {})
        xobjects = resources.get("/XObject", {})
        for reference in xobjects.values():
            if reference.get_object().get("/Subtype") == "/Image":
                count += 1
    return count


def dereference(value):
    return value.get_object() if isinstance(value, IndirectObject) else value


def structure_elements(value):
    value = dereference(value)
    if isinstance(value, list):
        for item in value:
            yield from structure_elements(item)
        return
    if not isinstance(value, DictionaryObject):
        return
    if value.get("/Type") == "/StructElem":
        yield value
    children = value.get("/K")
    if children is not None:
        yield from structure_elements(children)


def validate_accessibility(reader: PdfReader, image_count: int) -> None:
    root = reader.trailer["/Root"]
    mark_info = dereference(root.get("/MarkInfo"))
    if not mark_info or not bool(mark_info.get("/Marked")):
        raise ValueError("Guide PDF is not marked as a tagged document")
    if root.get("/Lang") != "en-GB":
        raise ValueError(f"Guide PDF language is not en-GB: {root.get('/Lang')!r}")

    structure_root = root.get("/StructTreeRoot")
    if structure_root is None:
        raise ValueError("Guide PDF has no structure tree")
    structure_root = dereference(structure_root)
    if structure_root.get("/ParentTree") is None:
        raise ValueError("Guide PDF structure tree has no parent tree")

    elements = list(structure_elements(structure_root.get("/K")))
    roles = [str(element.get("/S")) for element in elements]
    if roles.count("/H1") != 1:
        raise ValueError(f"Guide must contain exactly one H1 tag; found {roles.count('/H1')}")
    if roles.count("/H2") < len(REQUIRED_SECTIONS):
        raise ValueError(
            f"Guide has only {roles.count('/H2')} H2 tags; expected at least {len(REQUIRED_SECTIONS)}"
        )

    figures = [element for element in elements if element.get("/S") == "/Figure"]
    if len(figures) < image_count:
        raise ValueError(
            f"Guide has {image_count} images but only {len(figures)} Figure tags"
        )
    missing_alt = [element for element in figures if not str(element.get("/Alt", "")).strip()]
    if missing_alt:
        raise ValueError(f"Guide has {len(missing_alt)} Figure tags without alternative text")

    for page_number, page in enumerate(reader.pages, 1):
        if page.get("/StructParents") is None:
            raise ValueError(f"Guide page {page_number} has no StructParents entry")
        if page.get("/Tabs") != "/S":
            raise ValueError(f"Guide page {page_number} does not use structure tab order")


def validate_pdf(path: Path) -> None:
    if not path.is_file() or path.stat().st_size < 500_000:
        raise ValueError(f"Guide PDF is missing or unexpectedly small: {path}")

    reader = PdfReader(path)
    page_count = len(reader.pages)
    if page_count < 30:
        raise ValueError(f"Guide has only {page_count} pages; expected at least 30")

    metadata = reader.metadata
    if metadata.title != "Stashi Wallet User Guide":
        raise ValueError(f"Unexpected PDF title: {metadata.title!r}")
    if metadata.author != "Pirate Chain Foundation":
        raise ValueError(f"Unexpected PDF author: {metadata.author!r}")

    text = "\n".join(page.extract_text() or "" for page in reader.pages)
    missing_sections = [section for section in REQUIRED_SECTIONS if section not in text]
    if missing_sections:
        raise ValueError(f"Guide is missing sections: {missing_sections}")
    if "\ufffd" in text:
        raise ValueError("Guide contains Unicode replacement characters")

    titles = outline_titles(reader.outline)
    missing_bookmarks = [
        section for section in REQUIRED_SECTIONS[1:] if section not in titles
    ]
    if missing_bookmarks:
        raise ValueError(f"Guide is missing bookmarks: {missing_bookmarks}")

    link_count = count_annotations(reader, "/Link")
    image_count = count_images(reader)
    if link_count < 20:
        raise ValueError(f"Guide has only {link_count} links; expected at least 20")
    if image_count < 20:
        raise ValueError(f"Guide has only {image_count} images; expected at least 20")

    validate_accessibility(reader, image_count)

    print(
        f"Verified {path}: {page_count} pages, {len(titles)} bookmarks, "
        f"{link_count} links, {image_count} images"
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Validate the Stashi Wallet guide.")
    parser.add_argument("pdf", nargs="?", type=Path, default=DEFAULT_PDF)
    return parser.parse_args()


def main() -> None:
    validate_markdown_links()
    validate_editorial_language()
    validate_pdf(parse_args().pdf.resolve())


if __name__ == "__main__":
    main()
