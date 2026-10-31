"""Докачивает тяжёлые файлы (torch ~2.8 ГБ, Qwen3-4B ~8 ГБ) и ставит библиотеки — на компьютере для обучения.

Только стандартная библиотека Python. Список файлов с размерами и SHA-256 — manifest.json
(его пишет make_package.py). Каждый файл качается в <имя>.part с продолжением после обрыва
(HTTP Range); если источник недоступен — пробуется следующий (huggingface.co → hf-mirror.com →
modelscope.cn, pytorch.org → зеркало Alibaba). Готовый файл сверяется по SHA-256.
Мелкие библиотеки уже лежат в wheels/ и ставятся без интернета.

    python\\python.exe fetch.py
"""
import hashlib
import json
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
CHUNK = 1 << 20
TRIES = 5


def sha256(path):
    h = hashlib.sha256()
    with path.open("rb") as f:
        while b := f.read(16 * CHUNK):
            h.update(b)
    return h.hexdigest()


def fetch_once(url, part, size):
    have = part.stat().st_size if part.exists() else 0
    req = urllib.request.Request(url, headers={"Range": f"bytes={have}-", "User-Agent": "bookmind-fetch"})
    with urllib.request.urlopen(req, timeout=60) as r:
        if r.status == 200 and have:
            have = 0  # источник не умеет продолжать — качаем заново
        mode = "ab" if have else "wb"
        t0, got, shown = time.time(), 0, 0
        with part.open(mode) as f:
            while b := r.read(CHUNK):
                f.write(b)
                got += len(b)
                if time.time() - shown > 10:
                    shown = time.time()
                    speed = got / max(shown - t0, 1e-3)
                    left = (size - have - got) / max(speed, 1)
                    print(f"    {(have + got) / 1e9:.2f} из {size / 1e9:.2f} ГБ, "
                          f"{speed * 8 / 1e6:.1f} Мбит/с, осталось ~{left / 60:.0f} мин", flush=True)


def fetch(item):
    dest = HERE / item["path"]
    if dest.exists() and dest.stat().st_size == item["size"]:
        return
    dest.parent.mkdir(parents=True, exist_ok=True)
    part = dest.with_name(dest.name + ".part")
    print(f"качаю {item['path']} ({item['size'] / 1e9:.2f} ГБ)", flush=True)
    for url in item["urls"]:
        host = url.split("/")[2]
        for n in range(TRIES):
            try:
                if part.exists() and part.stat().st_size > item["size"]:
                    part.unlink()
                if not (part.exists() and part.stat().st_size == item["size"]):
                    fetch_once(url, part, item["size"])
                if part.stat().st_size != item["size"]:
                    raise IOError("соединение оборвалось")
                break
            except Exception as e:  # обрыв, таймаут, сайт закрыт
                print(f"    {host}: {e} — попытка {n + 2} из {TRIES}" if n + 1 < TRIES else f"    {host} не отвечает", flush=True)
                time.sleep(min(30, 5 * (n + 1)))
        if part.exists() and part.stat().st_size == item["size"]:
            break
    else:
        sys.exit(f"не удалось скачать {item['path']}: ни один источник не отдал файл целиком")
    print("    проверяю файл…", flush=True)
    if sha256(part) != item["sha256"]:
        part.unlink()
        sys.exit(f"{item['path']} скачался с ошибкой и удалён — запустите ещё раз")
    part.replace(dest)


def main():
    manifest = json.loads((HERE / "manifest.json").read_text(encoding="utf-8"))
    for item in manifest["files"]:
        fetch(item)
    done = HERE / "python" / "bookmind-installed"
    if done.exists():
        return
    print("ставлю библиотеки…", flush=True)
    py = [sys.executable, "-m"]
    subprocess.run(py + ["ensurepip"], check=True, capture_output=True)
    wheels = sorted(str(p) for p in (HERE / "wheels").glob("*.whl"))
    subprocess.run(py + ["pip", "install", "--no-index", "--no-deps", "--disable-pip-version-check", "--break-system-packages", "--no-warn-script-location",
                         "-q", *wheels, str(HERE / manifest["torch"])], check=True)
    done.write_text("ok")
    print("всё скачано и установлено", flush=True)


if __name__ == "__main__":
    main()
