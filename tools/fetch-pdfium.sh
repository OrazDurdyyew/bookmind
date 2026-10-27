#!/usr/bin/env bash
# Скачивает готовую сборку pdfium (bblanchon/pdfium-binaries) в tools/pdfium/.
set -euo pipefail
cd "$(dirname "$0")/.."
rm -rf tools/pdfium && mkdir -p tools/pdfium
curl -fL "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-linux-x64.tgz" | tar -xz -C tools/pdfium
echo "pdfium -> tools/pdfium"
