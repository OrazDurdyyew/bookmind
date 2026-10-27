#!/usr/bin/env bash
# Скачивает готовую сборку llama.cpp (llama-server) в tools/llama/<variant>/.
# Использование: tools/fetch-llama.sh [TAG] [VARIANT]
#   TAG     — тег релиза вида b11913 (по умолчанию — самый новый тег, где есть сборка VARIANT)
#   VARIANT — Linux: ubuntu-x64 (CPU, по умолчанию), ubuntu-vulkan-x64, ubuntu-cuda-12.8-x64 …
#             Windows: win-cpu-x64 (CPU, по умолчанию), win-vulkan-x64, win-cuda-12.4-x64 …
# CPU-сборка кладётся прямо в tools/llama/, остальные — в tools/llama/<variant>/.
set -euo pipefail
cd "$(dirname "$0")/.."
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) cpu="win-cpu-x64"; ext="zip"; exe=".exe" ;;
  *) cpu="ubuntu-x64"; ext="tar.gz"; exe="" ;;
esac
tag="${1:-}"
variant="${2:-$cpu}"
if [ -z "$tag" ]; then
  # releases/latest у llama.cpp может указывать на тег без сборок, поэтому ищем новейший b* с нужной сборкой.
  tag="$(curl -fsSL 'https://api.github.com/repos/ggml-org/llama.cpp/releases?per_page=20' \
    | grep -o "/download/b[0-9]*/llama-b[0-9]*-bin-$variant\.$ext\"" | head -1 | cut -d/ -f3)"
  [ -n "$tag" ] || { echo "нет сборки $variant в последних релизах" >&2; exit 1; }
fi
dest="tools/llama"; [ "$variant" = "$cpu" ] || dest="tools/llama/$variant"
tmp="$(mktemp -d)"
base="https://github.com/ggml-org/llama.cpp/releases/download/$tag"
curl -fsSL -o "$tmp/llama.$ext" "$base/llama-$tag-bin-$variant.$ext"
mkdir -p "$tmp/x"
if [ "$ext" = zip ]; then unzip -q "$tmp/llama.zip" -d "$tmp/x"; else tar -xzf "$tmp/llama.tar.gz" -C "$tmp/x"; fi
# Сборкам CUDA для Windows нужны DLL рантайма CUDA — они лежат отдельным архивом.
case "$variant" in
  win-cuda-*)
    curl -fsSL -o "$tmp/cudart.zip" "$base/cudart-llama-bin-$variant.zip"
    unzip -q -o "$tmp/cudart.zip" -d "$(dirname "$(find "$tmp/x" -name "llama-server$exe" -type f | head -1)")"
    ;;
esac
# Внутри архива файлы могут лежать во вложенной папке — переносим её содержимое.
bin="$(dirname "$(find "$tmp/x" -name "llama-server$exe" -type f | head -1)")"
mkdir -p "$dest" && find "$dest" -maxdepth 1 -type f -delete
cp -a "$bin"/. "$dest"/
echo "$tag" > "$dest/VERSION"
rm -rf "$tmp"
echo "llama.cpp $tag ($variant) -> $dest"
