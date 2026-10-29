"""Отбор фрагментов туркменских книг для примеров дообучения.

Вход — train/out/chunks.jsonl (пример `chunks` из bookmind-core), выход —
train/out/selected.jsonl: фрагменты с id, очищенные от кириллических букв-двойников.

    uv run --no-project python train/select_chunks.py

Правила:
- буквы-двойники (кириллические «а», «о», «ӧ», латинские «ñ», «ÿ»…) заменяются туркменскими;
- выбрасываются короткие куски, оглавления (много цифр и точек), куски с кириллицей;
- книги не на туркменском и проверочная книга не используются;
- словари — не больше ~8% примеров, одна книга — не больше MAX_PER_BOOK.
"""
import json
import random
import re
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "train/out/chunks.jsonl"
OUT = ROOT / "train/out/selected.jsonl"

TARGET = 1600
MAX_PER_BOOK = 110
DICTIONARY_SHARE = 0.08
SEED = 7

# Книги, которые не берём: английская грамматика и всё, на чём проверяем модель.
EXCLUDE = ["David Gray~Turkmen Grammar", "Halallyk kyssalary"]
DICTIONARIES = ["sözlügi", "Türkmen sözüniň gelip çykyşy"]

# Кириллица и чужая латиница, похожие на туркменские буквы.
HOMOGLYPHS = {
    "а": "a", "А": "A", "о": "o", "О": "O", "е": "e", "Е": "E", "р": "p", "Р": "P",
    "с": "c", "С": "C", "х": "h", "Х": "H", "у": "y", "к": "k", "К": "K", "м": "m",
    "М": "M", "т": "t", "Т": "T", "н": "n", "Н": "H", "в": "w", "В": "B", "і": "i",
    "ӧ": "ö", "Ӧ": "Ö", "ү": "ü", "Ү": "Ü", "ә": "ä", "Ә": "Ä", "ң": "ň", "Ң": "Ň",
    "җ": "j", "җ": "j", "ñ": "ň", "Ñ": "Ň", "ÿ": "ý", "Ÿ": "Ý", "ı": "y", "İ": "I",
    "ğ": "g", "Ğ": "G", "ş": "ş",
}
CYR = re.compile("[Ѐ-ӿ]")
LETTER = re.compile("[^\\W\\d_]")


def clean(text: str) -> str:
    """Заменяет двойники только в словах, где есть латиница: настоящие русские слова не трогаем."""
    def fix(m: re.Match) -> str:
        w = m.group(0)
        if re.search("[A-Za-zäöüçşňýžÄÖÜÇŞŇÝŽ]", w):
            return "".join(HOMOGLYPHS.get(ch, ch) for ch in w)
        return w
    return re.sub(r"\S+", fix, text)


def good(text: str) -> bool:
    words = text.split()
    if len(words) < 120:
        return False
    letters = LETTER.findall(text)
    if not letters:
        return False
    # Даже русская цитата учила бы модель смешивать алфавиты — такие куски не берём.
    if CYR.search(text):
        return False
    digits = sum(ch.isdigit() for ch in text)
    if digits / len(text) > 0.08 or text.count("...") + text.count("···") + text.count("…") > 8:
        return False
    return True


def main() -> None:
    by_book: dict[str, list[dict]] = defaultdict(list)
    for line in SRC.open(encoding="utf-8"):
        c = json.loads(line)
        if any(x in c["book"] for x in EXCLUDE):
            continue
        c["text"] = clean(c["text"])
        if good(c["text"]):
            by_book[c["book"]].append(c)

    rng = random.Random(SEED)
    is_dict = lambda b: any(x in b for x in DICTIONARIES)
    dict_books = [b for b in by_book if is_dict(b)]
    text_books = [b for b in by_book if not is_dict(b)]

    picked: list[dict] = []
    dict_quota = int(TARGET * DICTIONARY_SHARE)
    for b in dict_books:
        picked += rng.sample(by_book[b], min(len(by_book[b]), dict_quota // max(len(dict_books), 1)))

    # Остальное поровну между книгами, но не больше MAX_PER_BOOK; излишек — большим книгам.
    rest = TARGET - len(picked)
    quota = {b: 0 for b in text_books}
    while rest > 0 and any(quota[b] < min(len(by_book[b]), MAX_PER_BOOK) for b in text_books):
        for b in text_books:
            if rest > 0 and quota[b] < min(len(by_book[b]), MAX_PER_BOOK):
                quota[b] += 1
                rest -= 1
    for b in text_books:
        picked += rng.sample(by_book[b], quota[b])

    rng.shuffle(picked)
    with OUT.open("w", encoding="utf-8", newline="\n") as f:
        for i, c in enumerate(picked):
            c["id"] = f"tm-{i:04d}"
            f.write(json.dumps(c, ensure_ascii=False) + "\n")

    print(f"отобрано {len(picked)} из {sum(len(v) for v in by_book.values())} годных фрагментов")
    counts = defaultdict(int)
    for c in picked:
        counts[c["book"]] += 1
    for b, n in sorted(counts.items(), key=lambda x: -x[1]):
        print(f"  {n:4}  {b}")


if __name__ == "__main__":
    main()
