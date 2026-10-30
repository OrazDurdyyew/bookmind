"""Сборка примеров дообучения из ответов на фрагменты.

Вход: train/out/selected.jsonl, train/out/chunks.jsonl (все фрагменты — для «соседей»),
train/out/qa/batch-*.jsonl (вопрос, ответ, цитата по каждому фрагменту).
Выход: train/out/train.jsonl и train/out/val.jsonl — {"messages": [...]} в том же виде,
в каком программа говорит с моделью (см. build_answer_messages в crates/core/src/answer.rs).

    uv run --no-project python train/build_dataset.py

Каждый пример — 6 источников, как DEFAULT_TOP_K в поиске. Для вопроса с ответом это
фрагмент с ответом и 5 похожих фрагментов той же книги (BM25 по вопросу), в случайном
порядке; к ответу добавляется ссылка [n]. Для вопроса без ответа фрагмент с ответом и его
соседи убираются, правильный ответ — NOT_IN_BOOK.
"""
import json
import math
import random
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from select_chunks import clean  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "train/out"
TOP_K = 6
UNANSWERABLE_SHARE = 0.2
VAL_SHARE = 0.05
SEED = 11

# Книги, которых модель не увидит при обучении: по ним проверяем, научилась ли она
# навыку, а не запомнила обучающие книги. Их вопросы идут в holdout_qa.jsonl.
# (Не в EXCLUDE у select_chunks.py: перевыбор фрагментов сменил бы их id.)
HOLDOUT = ["Annamanow M~Astronomiýa", "Türkmen halk döredijiligi II"]

# Правки по ручной проверке 100 примеров (train/out/duzedilen.xlsx, 2026-10-30).
# Убраны: неясный вопрос, разные числа в источниках, примеры, отмеченные проверяющим как ошибочные.
DROP = {"tm-0420", "tm-0671", "tm-0197", "tm-0740", "tm-1005", "tm-0109", "tm-0073"}
REPLACE = {"tm-0856": [("roboty", "mehanizmi"), ("robot", "mehanizm")]}  # «robot» → «mehanizm»

FOREIGN = re.compile("[Ѐ-ӿıİğĞñÑÿŸ]")
WORD = re.compile(r"[^\W\d_]+")


def system_prompt_tm() -> str:
    """Туркменская инструкция — та же строка, что SYSTEM_PROMPT_TM в answer.rs."""
    src = (ROOT / "crates/core/src/answer.rs").read_text(encoding="utf-8")
    m = re.search(r'const SYSTEM_PROMPT_TM: &str = "(.*?)";', src, re.S)
    return m.group(1)


def label(c: dict) -> str:
    """Подпись источника, как source_label в answer.rs для туркменской книги."""
    parts = [c["chapter_title"] or f"Bölüm {c['chapter_index'] + 1}"]
    if c.get("subheading"):
        parts.append(c["subheading"])
    if c.get("page_start"):
        parts.append(f"sah. {c['page_start']}")
    return ", ".join(parts)


def user_message(sources: list[dict], question: str) -> str:
    s = "Çeşmeler:\n"
    for i, c in enumerate(sources):
        s += f"[{i + 1}] ({label(c)})\n{c['text']}\n\n"
    return s + f"Sorag: {question}"


def norm(s: str) -> str:
    return " ".join(s.split())


class Bm25:
    """Простой BM25 по фрагментам одной книги — «похожие» источники, как у настоящего поиска."""

    def __init__(self, docs: list[dict]):
        self.docs = docs
        self.tf = [Counter(w.lower() for w in WORD.findall(d["text"])) for d in docs]
        self.len = [sum(t.values()) for t in self.tf]
        self.avg = sum(self.len) / max(len(docs), 1)
        df = Counter(w for t in self.tf for w in t)
        n = len(docs)
        self.idf = {w: math.log(1 + (n - f + 0.5) / (f + 0.5)) for w, f in df.items()}

    def top(self, query: str, skip: set[int], k: int) -> list[dict]:
        q = [w.lower() for w in WORD.findall(query)]
        scored = []
        for i, t in enumerate(self.tf):
            if self.docs[i]["ord"] in skip:
                continue
            s = sum(self.idf.get(w, 0) * t[w] * 2.2 / (t[w] + 1.2 * (0.25 + 0.75 * self.len[i] / self.avg)) for w in q if w in t)
            scored.append((s, i))
        scored.sort(reverse=True)
        return [self.docs[i] for _, i in scored[:k]]


def main() -> None:
    selected = {}
    for line in (OUT / "selected.jsonl").open(encoding="utf-8"):
        c = json.loads(line)
        selected[c["id"]] = c

    by_book = defaultdict(list)
    for line in (OUT / "chunks.jsonl").open(encoding="utf-8"):
        c = json.loads(line)
        c["text"] = clean(c["text"])
        by_book[c["book"]].append(c)
    index = {b: Bm25(docs) for b, docs in by_book.items() if b in {c["book"] for c in selected.values()}}

    qa, problems = [], Counter()
    for path in sorted((OUT / "qa").glob("batch-*.jsonl")):
        for n, line in enumerate(path.open(encoding="utf-8"), 1):
            try:
                r = json.loads(line)
            except json.JSONDecodeError:
                problems["битый JSON"] += 1
                continue
            if "skip" in r:
                problems["пропущен"] += 1
                continue
            if r.get("id") in DROP:
                problems["убран при проверке"] += 1
                continue
            for old, new in REPLACE.get(r.get("id"), []):
                r["question"] = r["question"].replace(old, new)
                r["answer"] = r["answer"].replace(old, new)
            c = selected.get(r.get("id"))
            if not c or not r.get("question") or not r.get("answer") or not r.get("quote"):
                problems["нет полей или id"] += 1
                continue
            if FOREIGN.search(r["question"] + r["answer"]):
                problems["чужие буквы"] += 1
                continue
            if norm(r["quote"]) not in norm(c["text"]):
                problems["цитаты нет в тексте"] += 1
                continue
            if "[" in r["answer"]:
                r["answer"] = re.sub(r"\s*\[\d+\]", "", r["answer"])
            qa.append((r, c))

    rng = random.Random(SEED)
    rng.shuffle(qa)
    held = [(r, c) for r, c in qa if any(h in c["book"] for h in HOLDOUT)]
    qa = [(r, c) for r, c in qa if not any(h in c["book"] for h in HOLDOUT)]
    with (OUT / "holdout_qa.jsonl").open("w", encoding="utf-8", newline="\n") as f:
        for r, c in held:
            f.write(json.dumps({**r, "book": c["book"], "page_start": c["page_start"]}, ensure_ascii=False) + "\n")
    print(f"отложено для проверки: {len(held)} вопросов из {len(HOLDOUT)} книг")
    system = system_prompt_tm()
    examples = []
    for r, c in qa:
        neighbours = {c["ord"] - 1, c["ord"], c["ord"] + 1}
        unanswerable = rng.random() < UNANSWERABLE_SHARE
        others = index[c["book"]].top(r["question"], neighbours, TOP_K)
        # Цитата не должна встречаться в «чужих» источниках, иначе ответ там всё-таки есть.
        if any(norm(r["quote"]) in norm(o["text"]) for o in others):
            problems["цитата у соседа"] += 1
            continue
        if unanswerable:
            sources, reply = others[:TOP_K], "NOT_IN_BOOK"
        else:
            sources = others[: TOP_K - 1]
            pos = rng.randrange(len(sources) + 1)
            sources.insert(pos, c)
            reply = f"{r['answer'].strip()} [{pos + 1}]"
        examples.append({
            "id": r["id"], "unanswerable": unanswerable,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user_message(sources, r["question"])},
                {"role": "assistant", "content": reply},
            ],
        })

    n_val = max(1, int(len(examples) * VAL_SHARE))
    for name, part in [("val", examples[:n_val]), ("train", examples[n_val:])]:
        with (OUT / f"{name}.jsonl").open("w", encoding="utf-8", newline="\n") as f:
            for e in part:
                f.write(json.dumps(e, ensure_ascii=False) + "\n")

    print(f"примеров: {len(examples)} (без ответа: {sum(e['unanswerable'] for e in examples)}), "
          f"train {len(examples) - n_val}, val {n_val}")
    for k, v in problems.most_common():
        print(f"  {k}: {v}")


if __name__ == "__main__":
    main()
