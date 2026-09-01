from __future__ import annotations

import argparse
import html
import re
from pathlib import Path
from typing import Any

from PIL import Image as PILImage
from pypdf import PdfReader, PdfWriter
from pypdf.generic import (
    ArrayObject,
    BooleanObject,
    DecodedStreamObject,
    DictionaryObject,
    NameObject,
    NumberObject,
    TextStringObject,
)
from reportlab.lib import colors
from reportlab.lib.enums import TA_CENTER, TA_LEFT
from reportlab.lib.pagesizes import A4
from reportlab.lib.styles import ParagraphStyle, getSampleStyleSheet
from reportlab.lib.units import mm
from reportlab.platypus import (
    BaseDocTemplate,
    Frame,
    Image,
    KeepTogether,
    ListFlowable,
    ListItem,
    PageBreak,
    PageTemplate,
    Paragraph,
    Spacer,
    Table,
    TableStyle,
)
from reportlab.platypus.tableofcontents import TableOfContents


ROOT = Path(__file__).resolve().parents[2]
GUIDE = ROOT / "docs" / "user-guide"
DEFAULT_OUTPUT = ROOT / "output" / "pdf" / "Stashi-Wallet-User-Guide.pdf"

CHAPTERS = [
    ("getting-started.md", "Install and set up Stashi Wallet"),
    ("wallet-basics.md", "Wallet basics"),
    ("send-receive.md", "Receive and send ARRR"),
    ("keys-and-accounts.md", "Seed accounts, keys, and addresses"),
    ("migration.md", "Move from Treasure Chest or Pirate Wallet Lite"),
    ("network-and-sync.md", "Network privacy and synchronisation"),
    ("security-and-backups.md", "Backups and wallet security"),
    ("settings-and-verification.md", "Settings and release verification"),
    ("troubleshooting.md", "Troubleshooting"),
    ("advanced.md", "Advanced use"),
]


def slug(value: str) -> str:
    value = re.sub(r"<[^>]+>", "", value).lower()
    return re.sub(r"[^a-z0-9]+", "-", value).strip("-")


# PDF core fonts keep output stable across CI runners and developer machines.
BODY_FONT = "Helvetica"
MEDIUM_FONT = "Helvetica-Bold"
BOLD_FONT = "Helvetica-Bold"
MONO_FONT = "Courier"

INK = colors.HexColor("#111827")
MUTED = colors.HexColor("#58657A")
BLUE = colors.HexColor("#2F72F6")
LIGHT_BLUE = colors.HexColor("#EAF1FF")
LINE = colors.HexColor("#DDE4EF")
PAPER = colors.HexColor("#FFFFFF")
PANEL = colors.HexColor("#F6F8FC")


styles = getSampleStyleSheet()
styles.add(ParagraphStyle(
    name="GuideBody", fontName=BODY_FONT, fontSize=9.4, leading=14.1,
    textColor=INK, spaceAfter=6.5,
))


def _begin_accessibility_tag(
    canvas,
    role: str,
    *,
    alt_text: str | None = None,
    title: str | None = None,
) -> int:
    records: list[dict[str, Any]] = canvas._accessibility_records
    counters: dict[int, int] = canvas._accessibility_mcid_counters
    stack: list[int] = canvas._accessibility_tag_stack
    page_index = canvas.getPageNumber() - 1
    mcid = counters.get(page_index, 0)
    counters[page_index] = mcid + 1
    record_id = len(records)
    records.append(
        {
            "id": record_id,
            "page": page_index,
            "mcid": mcid,
            "role": role,
            "alt_text": alt_text,
            "title": title,
            "parent": stack[-1] if stack else None,
        }
    )
    stack.append(record_id)
    canvas._code.append(f"/{role} <</MCID {mcid}>> BDC")
    return record_id


def _end_accessibility_tag(canvas, record_id: int) -> None:
    stack: list[int] = canvas._accessibility_tag_stack
    if not stack or stack[-1] != record_id:
        raise RuntimeError("PDF accessibility tag stack is unbalanced")
    canvas._code.append("EMC")
    stack.pop()


class AccessibleParagraph(Paragraph):
    def __init__(
        self,
        *args,
        accessibility_role: str = "P",
        alt_text: str | None = None,
        structure_title: str | None = None,
        **kwargs,
    ):
        super().__init__(*args, **kwargs)
        self.accessibility_role = accessibility_role
        self.accessibility_alt_text = alt_text
        self.accessibility_title = structure_title

    def drawOn(self, canvas, x, y, _sW=0):
        record_id = _begin_accessibility_tag(
            canvas,
            self.accessibility_role,
            alt_text=self.accessibility_alt_text,
            title=self.accessibility_title,
        )
        try:
            return super().drawOn(canvas, x, y, _sW)
        finally:
            _end_accessibility_tag(canvas, record_id)

    def split(self, avail_width, avail_height):
        parts = super().split(avail_width, avail_height)
        for part in parts:
            if isinstance(part, AccessibleParagraph):
                part.accessibility_role = self.accessibility_role
                part.accessibility_alt_text = self.accessibility_alt_text
                part.accessibility_title = self.accessibility_title
        return parts


class AccessibleImage(Image):
    def __init__(self, *args, alt_text: str, **kwargs):
        super().__init__(*args, **kwargs)
        self.accessibility_alt_text = alt_text

    def drawOn(self, canvas, x, y, _sW=0):
        record_id = _begin_accessibility_tag(
            canvas,
            "Figure",
            alt_text=self.accessibility_alt_text,
        )
        try:
            return super().drawOn(canvas, x, y, _sW)
        finally:
            _end_accessibility_tag(canvas, record_id)


class AccessibleTable(Table):
    def drawOn(self, canvas, x, y, _sW=0):
        record_id = _begin_accessibility_tag(canvas, "Table")
        try:
            return super().drawOn(canvas, x, y, _sW)
        finally:
            _end_accessibility_tag(canvas, record_id)


class AccessibleListFlowable(ListFlowable):
    def drawOn(self, canvas, x, y, _sW=0):
        record_id = _begin_accessibility_tag(canvas, "L")
        try:
            return super().drawOn(canvas, x, y, _sW)
        finally:
            _end_accessibility_tag(canvas, record_id)


class AccessibleTableOfContents(TableOfContents):
    def drawOn(self, canvas, x, y, _sW=0):
        record_id = _begin_accessibility_tag(canvas, "TOC")
        try:
            return super().drawOn(canvas, x, y, _sW)
        finally:
            _end_accessibility_tag(canvas, record_id)
styles.add(ParagraphStyle(
    name="GuideSmall", parent=styles["GuideBody"], fontSize=7.7,
    leading=11.2, textColor=MUTED,
))
styles.add(ParagraphStyle(
    name="GuideH1", fontName=BOLD_FONT, fontSize=22, leading=27,
    textColor=INK, spaceAfter=12, keepWithNext=True,
))
styles.add(ParagraphStyle(
    name="GuideH2", fontName=BOLD_FONT, fontSize=14.5, leading=19,
    textColor=INK, spaceBefore=10, spaceAfter=7, keepWithNext=True,
))
styles.add(ParagraphStyle(
    name="GuideH3", fontName=MEDIUM_FONT, fontSize=11.2, leading=15,
    textColor=INK, spaceBefore=7, spaceAfter=4, keepWithNext=True,
))
styles.add(ParagraphStyle(
    name="GuideTable", parent=styles["GuideBody"], fontSize=7.8,
    leading=10.5, spaceAfter=0,
))
styles.add(ParagraphStyle(
    name="GuideCaption", parent=styles["GuideSmall"], alignment=TA_CENTER,
    fontName=MEDIUM_FONT, textColor=INK, spaceAfter=3,
))
styles.add(ParagraphStyle(
    name="GuideCode", fontName=MONO_FONT, fontSize=7.5, leading=10.5,
    backColor=PANEL, borderColor=LINE, borderWidth=0.5, borderPadding=6,
    spaceBefore=3, spaceAfter=7,
))
styles.add(ParagraphStyle(
    name="CoverTitle", fontName=BOLD_FONT, fontSize=31, leading=36,
    textColor=INK, alignment=TA_CENTER, spaceAfter=10,
))
styles.add(ParagraphStyle(
    name="CoverSub", fontName=BODY_FONT, fontSize=12, leading=18,
    textColor=MUTED, alignment=TA_CENTER,
))
styles.add(ParagraphStyle(
    name="TocTitle", fontName=BOLD_FONT, fontSize=24, leading=30,
    textColor=INK, spaceAfter=14,
))


def anchor_for(file_name: str, heading: str | None = None) -> str:
    base = Path(file_name).stem
    return f"chapter-{slug(base)}" if heading is None else f"{slug(base)}-{slug(heading)}"


ANCHORS: dict[str, str] = {}
for file_name, _ in CHAPTERS:
    ANCHORS[file_name] = anchor_for(file_name)
    for line in (GUIDE / file_name).read_text(encoding="utf-8").splitlines():
        if line.startswith("## "):
            title = line[3:].strip()
            ANCHORS[f"{file_name}#{slug(title)}"] = anchor_for(file_name, title)


def inline(text: str, current_file: str) -> str:
    tokens: list[str] = []

    def protect(value: str) -> str:
        tokens.append(value)
        return f"@@TOKEN{len(tokens) - 1}@@"

    text = re.sub(r"`([^`]+)`", lambda m: protect(f'<font name="{MONO_FONT}">{html.escape(m.group(1))}</font>'), text)
    text = html.escape(text, quote=False)
    text = re.sub(r"\*\*([^*]+)\*\*", rf'<font name="{BOLD_FONT}">\1</font>', text)

    def link(match: re.Match[str]) -> str:
        label, target = match.group(1), match.group(2)
        if target.startswith(("http://", "https://")):
            href = target
        else:
            file_part, _, fragment = target.partition("#")
            target_file = file_part or current_file
            if target_file == "../verify-build.md":
                target_file = "settings-and-verification.md"
                fragment = "verify-the-downloaded-release-files"
            key = f"{target_file}#{slug(fragment)}" if fragment else target_file
            href = f"#{ANCHORS.get(key, anchor_for(target_file, fragment or None))}"
        return protect(f'<link href="{html.escape(href)}" color="#2F72F6">{html.escape(label)}</link>')

    text = re.sub(r"\[([^\]]+)\]\(([^)]+)\)", link, text)
    for index, value in enumerate(tokens):
        text = text.replace(f"@@TOKEN{index}@@", value)
    return text


def image_flowable(
    relative: str,
    cell_width: float,
    *,
    alt_text: str,
    max_height: float = 108 * mm,
):
    path = GUIDE / relative
    with PILImage.open(path) as source:
        width, height = source.size
    ratio = width / height
    if ratio < 0.75:
        draw_width = min(cell_width * 0.53, 45 * mm)
    else:
        draw_width = cell_width - 5 * mm
    draw_height = draw_width / ratio
    if draw_height > max_height:
        draw_height = max_height
        draw_width = draw_height * ratio
    result = AccessibleImage(
        str(path),
        width=draw_width,
        height=draw_height,
        alt_text=alt_text,
    )
    result.hAlign = "CENTER"
    return result


def parse_table(lines: list[str], start: int, current_file: str):
    raw_rows = []
    index = start
    while index < len(lines) and lines[index].strip().startswith("|"):
        raw_rows.append([cell.strip() for cell in lines[index].strip().strip("|").split("|")])
        index += 1
    if len(raw_rows) > 1 and all(re.fullmatch(r":?-{3,}:?", cell) for cell in raw_rows[1]):
        raw_rows.pop(1)
    column_count = max(len(row) for row in raw_rows)
    page_width = A4[0] - 36 * mm
    column_widths = [page_width / column_count] * column_count
    rows = []
    image_table = any("![" in cell for row in raw_rows for cell in row)
    for row_index, row in enumerate(raw_rows):
        output_row = []
        for column, cell in enumerate(row):
            match = re.fullmatch(r"!\[([^]]*)\]\(([^)]+)\)", cell)
            if match:
                output_row.append(
                    image_flowable(
                        match.group(2),
                        column_widths[column],
                        alt_text=match.group(1),
                    )
                )
            else:
                style = styles["GuideCaption"] if image_table and row_index == 0 else styles["GuideTable"]
                output_row.append(
                    AccessibleParagraph(
                        inline(cell, current_file),
                        style,
                        accessibility_role="TH" if row_index == 0 else "TD",
                    )
                )
        output_row.extend([""] * (column_count - len(output_row)))
        rows.append(output_row)
    table = AccessibleTable(
        rows,
        colWidths=column_widths,
        repeatRows=1,
        hAlign="LEFT",
    )
    table.setStyle(TableStyle([
        ("BACKGROUND", (0, 0), (-1, 0), LIGHT_BLUE if not image_table else PANEL),
        ("TEXTCOLOR", (0, 0), (-1, -1), INK),
        ("GRID", (0, 0), (-1, -1), 0.5, LINE),
        ("VALIGN", (0, 0), (-1, -1), "MIDDLE"),
        ("ALIGN", (0, 0), (-1, -1), "CENTER" if image_table else "LEFT"),
        ("LEFTPADDING", (0, 0), (-1, -1), 5),
        ("RIGHTPADDING", (0, 0), (-1, -1), 5),
        ("TOPPADDING", (0, 0), (-1, -1), 5),
        ("BOTTOMPADDING", (0, 0), (-1, -1), 5),
    ]))
    return table, index


def _list_flowables(
    records: list[tuple[bool, int, str]],
    current_file: str,
) -> list[AccessibleListFlowable]:
    def build_level(position: int, indent: int):
        ordered = records[position][0]
        items: list[ListItem] = []
        while position < len(records):
            item_ordered, item_indent, value = records[position]
            if item_indent < indent or (item_indent == indent and item_ordered != ordered):
                break
            if item_indent > indent:
                nested, position = build_level(position, item_indent)
                if not items:
                    raise ValueError("Nested list item has no parent")
                items[-1]._flowables = tuple(items[-1]._flowables) + (nested,)
                continue
            item_content = AccessibleParagraph(
                inline(value, current_file),
                styles["GuideBody"],
                accessibility_role="LI",
            )
            items.append(ListItem(item_content, leftIndent=4))
            position += 1

        options = (
            {"bulletType": "1", "start": 1}
            if ordered
            else {"bulletType": "bullet", "start": "\u2022"}
        )
        return (
            AccessibleListFlowable(
                items,
                leftIndent=17,
                bulletFontName=BODY_FONT,
                bulletFontSize=8.5,
                spaceAfter=6,
                **options,
            ),
            position,
        )

    result: list[AccessibleListFlowable] = []
    position = 0
    while position < len(records):
        flowable, position = build_level(position, records[position][1])
        result.append(flowable)
    return result


def parse_markdown(file_name: str, *, include_h1: bool = True):
    lines = (GUIDE / file_name).read_text(encoding="utf-8").splitlines()
    story = []
    index = 0
    list_items: list[tuple[bool, int, str]] = []
    code_lines: list[str] = []
    in_code = False

    def flush_list():
        nonlocal list_items
        if not list_items:
            return
        story.extend(_list_flowables(list_items, file_name))
        list_items = []

    while index < len(lines):
        line = lines[index].rstrip()
        stripped = line.strip()
        if stripped.startswith("```"):
            flush_list()
            if in_code:
                story.append(
                    AccessibleParagraph(
                        "<br/>".join(html.escape(item) for item in code_lines),
                        styles["GuideCode"],
                        accessibility_role="Code",
                    )
                )
                code_lines = []
            in_code = not in_code
            index += 1
            continue
        if in_code:
            code_lines.append(line)
            index += 1
            continue
        if not stripped:
            flush_list()
            index += 1
            continue
        if stripped == "<!-- page-break -->":
            flush_list()
            story.append(PageBreak())
            index += 1
            continue
        if stripped.startswith("|") and index + 1 < len(lines) and lines[index + 1].strip().startswith("|"):
            flush_list()
            table, index = parse_table(lines, index, file_name)
            story.append(KeepTogether([table, Spacer(1, 7)]))
            continue
        image_match = re.fullmatch(r"!\[([^]]*)\]\(([^)]+)\)", stripped)
        if image_match:
            flush_list()
            story.append(
                KeepTogether(
                    [
                        AccessibleParagraph(
                            inline(image_match.group(1), file_name),
                            styles["GuideCaption"],
                            accessibility_role="Caption",
                        ),
                        image_flowable(
                            image_match.group(2),
                            A4[0] - 36 * mm,
                            alt_text=image_match.group(1),
                        ),
                        Spacer(1, 7),
                    ]
                )
            )
            index += 1
            continue
        heading = re.match(r"^(#{1,3})\s+(.+)$", stripped)
        if heading:
            flush_list()
            level = len(heading.group(1))
            title = heading.group(2).strip()
            if level == 1 and not include_h1:
                index += 1
                continue
            anchor = anchor_for(file_name) if level == 1 else anchor_for(file_name, title)
            paragraph = AccessibleParagraph(
                inline(title, file_name),
                styles[f"GuideH{level}"],
                accessibility_role=f"H{level + 1}",
                structure_title=title,
            )
            paragraph._bookmarkName = anchor
            paragraph._outlineLevel = level - 1
            paragraph._tocText = title
            story.append(paragraph)
            index += 1
            continue
        list_match = re.match(r"^(\s*)(\d+\.|-)\s+(.+)$", line)
        if list_match:
            is_ordered = list_match.group(2) != "-"
            indent = len(list_match.group(1).expandtabs(2))
            value = list_match.group(3)
            list_items.append((is_ordered, indent, value))
            index += 1
            continue
        flush_list()
        paragraph_lines = [stripped]
        index += 1
        while index < len(lines):
            candidate = lines[index].strip()
            if not candidate or candidate.startswith(("#", "|", "- ", "```", "![")) or re.match(r"^\d+\.\s", candidate):
                break
            paragraph_lines.append(candidate)
            index += 1
        story.append(
            AccessibleParagraph(
                inline(" ".join(paragraph_lines), file_name),
                styles["GuideBody"],
            )
        )
    flush_list()
    return story


class GuideDocTemplate(BaseDocTemplate):
    def __init__(self, filename: str):
        super().__init__(
            filename,
            pagesize=A4,
            leftMargin=18 * mm,
            rightMargin=18 * mm,
            topMargin=18 * mm,
            bottomMargin=17 * mm,
            title="Stashi Wallet User Guide",
            author="Pirate Chain Foundation",
            subject="Complete user guide for Stashi Wallet",
            creator="Pirate Chain Foundation",
        )
        frame = Frame(self.leftMargin, self.bottomMargin, self.width, self.height, id="normal")
        self.addPageTemplates(PageTemplate(id="guide", frames=[frame], onPage=self.draw_page))

    def beforeDocument(self):
        super().beforeDocument()
        self._accessibility_records: list[dict[str, Any]] = []
        self.canv._accessibility_records = self._accessibility_records
        self.canv._accessibility_mcid_counters = {}
        self.canv._accessibility_tag_stack = []

    def draw_page(self, canvas, doc):
        if doc.page <= 1:
            return
        canvas._code.append("/Artifact BMC")
        canvas.saveState()
        canvas.setStrokeColor(LINE)
        canvas.setLineWidth(0.5)
        canvas.line(18 * mm, 13 * mm, A4[0] - 18 * mm, 13 * mm)
        canvas.setFont(BODY_FONT, 7.5)
        canvas.setFillColor(MUTED)
        canvas.drawString(18 * mm, 8.7 * mm, "Stashi Wallet user guide")
        canvas.drawRightString(A4[0] - 18 * mm, 8.7 * mm, str(doc.page))
        canvas.restoreState()
        canvas._code.append("EMC")

    def afterFlowable(self, flowable):
        bookmark = getattr(flowable, "_bookmarkName", None)
        if not bookmark:
            return
        level = getattr(flowable, "_outlineLevel", 0)
        title = getattr(flowable, "_tocText", "")
        self.canv.bookmarkPage(bookmark)
        self.canv.addOutlineEntry(title, bookmark, level=max(0, min(level, 2)), closed=level > 0)
        if level <= 1:
            self.notify("TOCEntry", (level, title, self.page, bookmark))


def build_story():
    story = []
    logo = ROOT / "app" / "assets" / "icons" / "stashi-wallet-logo.png"
    story.extend([
        Spacer(1, 26 * mm),
        AccessibleImage(
            str(logo),
            width=42 * mm,
            height=42 * mm,
            alt_text="Stashi Wallet logo",
        ),
        Spacer(1, 10 * mm),
        AccessibleParagraph(
            "Stashi Wallet",
            styles["CoverTitle"],
            accessibility_role="H1",
            structure_title="Stashi Wallet",
        ),
        AccessibleParagraph("User guide", styles["CoverSub"]),
        Spacer(1, 10 * mm),
        AccessibleParagraph(
            "Setup, recovery, payments, privacy, key management, verification, and troubleshooting.",
            styles["CoverSub"],
        ),
        Spacer(1, 50 * mm),
        AccessibleParagraph("Mobile and desktop", styles["GuideCaption"]),
        AccessibleParagraph("Pirate Chain Foundation", styles["CoverSub"]),
        PageBreak(),
        AccessibleParagraph(
            "Contents",
            styles["TocTitle"],
            accessibility_role="H2",
            structure_title="Contents",
        ),
    ])
    toc = AccessibleTableOfContents()
    toc.levelStyles = [
        ParagraphStyle(name="TOC1", fontName=MEDIUM_FONT, fontSize=10, leading=15, leftIndent=0, firstLineIndent=0, textColor=INK, spaceBefore=3),
        ParagraphStyle(name="TOC2", fontName=BODY_FONT, fontSize=8.2, leading=12, leftIndent=12, firstLineIndent=0, textColor=MUTED),
    ]
    story.extend([toc, PageBreak()])

    intro = parse_markdown("README.md")
    filtered = []
    skip_contents = False
    for item in intro:
        if isinstance(item, Paragraph) and getattr(item, "_tocText", "") == "Contents":
            skip_contents = True
            continue
        if skip_contents:
            if isinstance(item, Paragraph) and getattr(item, "_outlineLevel", 99) == 1:
                skip_contents = False
            else:
                continue
        filtered.append(item)
    story.extend(filtered)

    for chapter_index, (file_name, _) in enumerate(CHAPTERS):
        story.append(PageBreak())
        story.extend(parse_markdown(file_name))
    return story


def add_accessibility_structure(
    source: Path,
    destination: Path,
    records: list[dict[str, Any]],
) -> None:
    if not records:
        raise ValueError("The guide did not produce any accessibility tags")

    reader = PdfReader(source)
    writer = PdfWriter()
    writer.clone_document_from_reader(reader)

    records = [dict(record) for record in records]
    tagged_pages = {record["page"] for record in records}
    for page_index, page in enumerate(writer.pages):
        if page_index in tagged_pages:
            continue

        prefix = DecodedStreamObject()
        prefix.set_data(b"/TOC <</MCID 0>> BDC\n")
        suffix = DecodedStreamObject()
        suffix.set_data(b"\nEMC\n")
        existing_contents = page.get("/Contents")
        if isinstance(existing_contents, ArrayObject):
            content_parts = list(existing_contents)
        elif existing_contents is None:
            content_parts = []
        else:
            content_parts = [existing_contents]
        page[NameObject("/Contents")] = ArrayObject(
            [
                writer._add_object(prefix),
                *content_parts,
                writer._add_object(suffix),
            ]
        )
        records.append(
            {
                "id": len(records),
                "page": page_index,
                "mcid": 0,
                "role": "TOC",
                "alt_text": None,
                "title": "Contents continued",
                "parent": None,
            }
        )

    structure_root = DictionaryObject(
        {
            NameObject("/Type"): NameObject("/StructTreeRoot"),
        }
    )
    structure_root_ref = writer._add_object(structure_root)
    document_element = DictionaryObject(
        {
            NameObject("/Type"): NameObject("/StructElem"),
            NameObject("/S"): NameObject("/Document"),
            NameObject("/P"): structure_root_ref,
        }
    )
    document_element_ref = writer._add_object(document_element)

    element_refs = []
    for record in records:
        page_index = record["page"]
        if page_index < 0 or page_index >= len(writer.pages):
            raise ValueError(f"Accessibility tag references invalid page {page_index + 1}")
        element = DictionaryObject(
            {
                NameObject("/Type"): NameObject("/StructElem"),
                NameObject("/S"): NameObject(f"/{record['role']}"),
                NameObject("/Pg"): writer.pages[page_index].indirect_reference,
            }
        )
        if record.get("alt_text"):
            element[NameObject("/Alt")] = TextStringObject(record["alt_text"])
        if record.get("title"):
            element[NameObject("/T")] = TextStringObject(record["title"])
        element_refs.append(writer._add_object(element))

    child_ids: dict[int, list[int]] = {record["id"]: [] for record in records}
    top_level_ids: list[int] = []
    for record in records:
        parent_id = record["parent"]
        if parent_id is None:
            top_level_ids.append(record["id"])
        else:
            child_ids[parent_id].append(record["id"])

    for record, element_ref in zip(records, element_refs, strict=True):
        element = element_ref.get_object()
        parent_id = record["parent"]
        element[NameObject("/P")] = (
            document_element_ref if parent_id is None else element_refs[parent_id]
        )
        contents = ArrayObject([NumberObject(record["mcid"])])
        contents.extend(element_refs[child_id] for child_id in child_ids[record["id"]])
        element[NameObject("/K")] = contents[0] if len(contents) == 1 else contents

    document_element[NameObject("/K")] = ArrayObject(
        [element_refs[record_id] for record_id in top_level_ids]
    )
    structure_root[NameObject("/K")] = ArrayObject([document_element_ref])

    records_by_page: dict[int, list[dict[str, Any]]] = {}
    for record in records:
        records_by_page.setdefault(record["page"], []).append(record)

    parent_tree_numbers = ArrayObject()
    for page_index, page_records in sorted(records_by_page.items()):
        page_records.sort(key=lambda item: item["mcid"])
        expected_mcids = list(range(len(page_records)))
        actual_mcids = [item["mcid"] for item in page_records]
        if actual_mcids != expected_mcids:
            raise ValueError(
                f"Page {page_index + 1} has non-contiguous marked-content identifiers"
            )
        writer.pages[page_index][NameObject("/StructParents")] = NumberObject(
            page_index
        )
        writer.pages[page_index][NameObject("/Tabs")] = NameObject("/S")
        parent_tree_numbers.extend(
            [
                NumberObject(page_index),
                ArrayObject([element_refs[item["id"]] for item in page_records]),
            ]
        )

    parent_tree = DictionaryObject(
        {
            NameObject("/Nums"): parent_tree_numbers,
        }
    )
    structure_root[NameObject("/ParentTree")] = writer._add_object(parent_tree)
    structure_root[NameObject("/ParentTreeNextKey")] = NumberObject(
        max(records_by_page) + 1
    )

    root = writer.root_object
    root[NameObject("/StructTreeRoot")] = structure_root_ref
    root[NameObject("/MarkInfo")] = DictionaryObject(
        {NameObject("/Marked"): BooleanObject(True)}
    )
    root[NameObject("/Lang")] = TextStringObject("en-GB")
    root[NameObject("/ViewerPreferences")] = DictionaryObject(
        {NameObject("/DisplayDocTitle"): BooleanObject(True)}
    )

    with destination.open("wb") as output_stream:
        writer.write(output_stream)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build the Stashi Wallet user guide PDF.")
    parser.add_argument(
        "--output",
        type=Path,
        default=DEFAULT_OUTPUT,
        help="Destination PDF path (default: output/pdf/Stashi-Wallet-User-Guide.pdf)",
    )
    return parser.parse_args()


def main():
    output = parse_args().output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary_directory = ROOT / "tmp" / "pdfs"
    temporary_directory.mkdir(parents=True, exist_ok=True)
    intermediate = temporary_directory / "Stashi-Wallet-User-Guide-untagged.pdf"
    doc = GuideDocTemplate(str(intermediate))
    try:
        doc.multiBuild(build_story())
        add_accessibility_structure(intermediate, output, doc._accessibility_records)
    finally:
        intermediate.unlink(missing_ok=True)
    print(output)


if __name__ == "__main__":
    main()
