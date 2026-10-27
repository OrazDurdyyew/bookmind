#!/usr/bin/env bash
# Скачивает готовую сборку llama.cpp (llama-server) в tools/llama/<variant>/.
# Использование: tools/fetch-llama.sh [TAG] [VARIANT]
#   TAG     — тег релиза вида b11913 (по умолчанию — самый новый тег b*)
#   VARIANT — ubuntu-x64 (CPU, по умолчанию), ubuntu-vulkan-x64, ubuntu-cuda-12.8-x64 …
# CPU-сборка кладётся прямо в tools/llama/, остальные — в tools/llama/<variant>/.
set -euo pipefail
cd "$(dirname "$0")/.."
tag="${1:-}"
variant="${2:-ubuntu-x64}"
if [ -z "$tag" ]; then
  # releases/latest у llama.cpp может указывать на тег без сборок, поэтому берём новейший b*.
  tag="$(curl -fsSL 'https://api.github.com/repos/ggml-org/llama.cpp/releases?per_page=10' \
    | python3 -c 'import json,sys; print(next(r["tag_name"] for r in json.load(sys.stdin) if r["tag_name"].startswith("b") and r["assets"]))')"
fi
dest="tools/llama"; [ "$variant" = "ubuntu-x64" ] || dest="tools/llama/$variant"
tmp="$(mktemp -d)"
curl -fsSL -o "$tmp/llama.tar.gz" "https://github.com/ggml-org/llama.cpp/releases/download/$tag/llama-$tag-bin-$variant.tar.gz"
mkdir -p "$tmp/x" && tar -xzf "$tmp/llama.tar.gz" -C "$tmp/x"
# Внутри архива файлы лежат во вложенной папке — переносим её содержимое.
bin="$(dirname "$(find "$tmp/x" -name llama-server -type f | head -1)")"
mkdir -p "$dest" && find "$dest" -maxdepth 1 -type f -delete
cp -a "$bin"/. "$dest"/
echo "$tag" > "$dest/VERSION"
rm -rf "$tmp"
echo "llama.cpp $tag ($variant) -> $dest"
