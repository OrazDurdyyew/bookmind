"""Создаёт тестовые PDF для crates/core/tests/fixtures.

Запуск (во временном окружении):
    uv run --with reportlab --with pypdf python tools/make-pdf-fixtures.py
"""
import io
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from reportlab.lib.pagesizes import A5
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.ttfonts import TTFont
from reportlab.pdfgen import canvas

OUT = Path(__file__).resolve().parent.parent / "crates/core/tests/fixtures"
FILLER = "Это учебный текст для проверки извлечения страниц из PDF."

PAGES = [
    ["Предисловие.", "Эта книга рассказывает о растениях.", FILLER],
    ["Оглавление: первая глава, вторая глава.", FILLER],
    ["Глава 1", "Растения используют солнечный свет.", FILLER],
    ["Процесс называется фото-", "синтез и идёт в листьях.", FILLER],
    ["Глава 2", "Животные двигаются и питаются.", FILLER],
    ["Конец книги.", FILLER],
]


def make_sample() -> None:
    pdfmetrics.registerFont(TTFont("DejaVu", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"))
    buf = io.BytesIO()
    c = canvas.Canvas(buf, pagesize=A5)
    for lines in PAGES:
        c.setFont("DejaVu", 11)
        y = 520
        for line in lines:
            c.drawString(40, y, line)
            y -= 16
        c.showPage()
    c.save()

    writer = PdfWriter(clone_from=PdfReader(io.BytesIO(buf.getvalue())))
    # Метки страниц: i, ii, затем 1..4.
    writer.set_page_label(0, 1, style="/r")
    writer.set_page_label(2, 5, style="/D", start=1)
    writer.add_outline_item("Глава 1", 2)
    writer.add_outline_item("Глава 2", 4)
    with open(OUT / "sample.pdf", "wb") as f:
        writer.write(f)


def make_scanned() -> None:
    buf = OUT / "scanned.pdf"
    c = canvas.Canvas(str(buf), pagesize=A5)
    for _ in range(3):
        c.rect(50, 50, 300, 400, fill=1)
        c.showPage()
    c.save()


if __name__ == "__main__":
    make_sample()
    make_scanned()
    print("ok:", OUT / "sample.pdf", OUT / "scanned.pdf")
