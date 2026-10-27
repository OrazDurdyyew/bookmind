#!/usr/bin/env bash
# Скачивает готовую сборку pdfium (bblanchon/pdfium-binaries) в tools/pdfium/.
# В Linux библиотека ложится в tools/pdfium/lib/, в Windows pdfium.dll — в tools/pdfium/bin/.
set -euo pipefail
cd "$(dirname "$0")/.."
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) platform="win-x64" ;;
  *) platform="linux-x64" ;;
esac
rm -rf tools/pdfium && mkdir -p tools/pdfium
curl -fL "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-$platform.tgz" | tar -xz -C tools/pdfium
echo "pdfium ($platform) -> tools/pdfium"
