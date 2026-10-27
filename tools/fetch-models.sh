#!/usr/bin/env bash
# Скачивает модели из eval/models.toml в models/.
# Использование: tools/fetch-models.sh [name ...]   (без имён — все модели)
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p models
# В Windows python3 — заглушка Microsoft Store, поэтому сначала пробуем uv.
py() { if command -v uv >/dev/null; then uv run --no-project python "$@"; else python3 "$@"; fi; }
py - "$@" <<'PY' | tr -d '\r' | while read -r file url; do
import sys, tomllib
cfg = tomllib.load(open("eval/models.toml", "rb"))
entries = cfg["candidates"] + cfg["embedders"] + [cfg["judge"]]
wanted = set(sys.argv[1:])
seen = set()
for e in entries:
    if (not wanted or e["name"] in wanted) and e["file"] not in seen:
        seen.add(e["file"])
        print(e["file"], e["url"])
PY
  if [ -f "models/$file" ] && [ ! -f "models/$file.part" ]; then
    echo "есть: $file"
    continue
  fi
  echo "качаю: $file"
  touch "models/$file.part"
  curl -fL -C - --retry 3 -o "models/$file" "$url"
  rm -f "models/$file.part"
done
