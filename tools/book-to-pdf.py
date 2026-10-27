"""Вёрстка текста книги в PDF для набора проверки.

Вход — текст в формате `inspect dump`: строки «## Название главы», затем абзацы
через пустую строку. Выход — PDF A5: титульная страница с меткой «i», дальше
страницы 1, 2, 3… (номер напечатан внизу), закладка на каждую главу.

    uv run --no-project --with reportlab --with pypdf python tools/book-to-pdf.py in.txt out.pdf "Название"
"""
import io
import sys

from pypdf import PdfReader, PdfWriter
from reportlab.lib.pagesizes import A5
from reportlab.lib.styles import ParagraphStyle
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.ttfonts import TTFont
from reportlab.platypus import BaseDocTemplate, Frame, PageBreak, PageTemplate, Paragraph, Spacer
from xml.sax.saxutils import escape

FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"
FONT_BOLD = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"


def parse(path):
    chapters, current = [], None
    for block in open(path, encoding="utf-8").read().split("\n\n"):
        block = block.strip()
        if not block:
            continue
        if block.startswith("## "):
            current = [block[3:].strip(), []]
            chapters.append(current)
        elif current is not None:
            current[1].append(block)
    return chapters


def main(src, dst, title):
    pdfmetrics.registerFont(TTFont("DejaVu", FONT))
    pdfmetrics.registerFont(TTFont("DejaVu-Bold", FONT_BOLD))
    body = ParagraphStyle("body", fontName="DejaVu", fontSize=9.5, leading=13, spaceAfter=5, firstLineIndent=12)
    head = ParagraphStyle("head", fontName="DejaVu-Bold", fontSize=13, leading=17, spaceAfter=10, spaceBefore=6)
    big = ParagraphStyle("big", fontName="DejaVu-Bold", fontSize=20, leading=26, alignment=1)

    starts = []  # (название главы, индекс страницы)

    def footer(canvas, doc):
        page = canvas.getPageNumber()
        if page > 1:
            canvas.setFont("DejaVu", 8)
            canvas.drawCentredString(A5[0] / 2, 20, str(page - 1))

    class Doc(BaseDocTemplate):
        def afterFlowable(self, f):
            if getattr(f, "chapter_title", None):
                starts.append((f.chapter_title, self.page - 1))

    buf = io.BytesIO()
    doc = Doc(buf, pagesize=A5, leftMargin=36, rightMargin=36, topMargin=36, bottomMargin=40)
    doc.addPageTemplates([PageTemplate(frames=[Frame(36, 40, A5[0] - 72, A5[1] - 76)], onPage=footer)])
    story = [Spacer(1, 150), Paragraph(escape(title), big), PageBreak()]
    for name, paras in parse(src):
        h = Paragraph(escape(name), head)
        h.chapter_title = name
        story.append(h)
        story.extend(Paragraph(escape(p), body) for p in paras)
        story.append(PageBreak())
    doc.build(story)

    writer = PdfWriter(clone_from=PdfReader(io.BytesIO(buf.getvalue())))
    n = len(writer.pages)
    writer.set_page_label(0, 0, style="/r")
    writer.set_page_label(1, n - 1, style="/D", start=1)
    for name, page in starts:
        writer.add_outline_item(name, page)
    with open(dst, "wb") as f:
        writer.write(f)
    print(f"{dst}: {n} страниц, {len(starts)} глав")


if __name__ == "__main__":
    main(*sys.argv[1:4])
