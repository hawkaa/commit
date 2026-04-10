#!/usr/bin/env bash
# Package the Commit extension for Chrome Web Store upload
set -euo pipefail

cd "$(dirname "$0")"

OUT="commit-extension.zip"
rm -f "$OUT"

zip -r "$OUT" \
  manifest.json \
  background.js \
  content-github.js \
  content-google.js \
  trust-card.css \
  icons/icon-16.png \
  icons/icon-48.png \
  icons/icon-128.png

echo "Packaged: $OUT ($(du -h "$OUT" | cut -f1))"
