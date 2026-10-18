#!/usr/bin/env bash
# Download the pinned libpdfium for this machine into a directory, for bundling
# into the desktop app (see crates/latex_ai_tauri/tauri.conf.json) or for a
# local install. Usage: scripts/fetch-pdfium.sh [DEST]
#
# DEST defaults to crates/latex_ai_tauri/resources/pdfium, which is what the
# Tauri bundle picks up. The archive is checked against a pinned SHA-256 where
# one is recorded; for other platforms the script says so instead of pretending.
set -euo pipefail

VERSION="${PDFIUM_VERSION:-chromium/7961}"
DEST="${1:-$(dirname "$0")/../crates/latex_ai_tauri/resources/pdfium}"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)  ASSET=pdfium-linux-x64;   LIB=lib/libpdfium.so;
                 SHA=019665c8877d46fe65f625f80fd714ab07aac68554b0636acf2a2adf9288adb2 ;;
  Linux-aarch64) ASSET=pdfium-linux-arm64; LIB=lib/libpdfium.so;    SHA= ;;
  Darwin-x86_64) ASSET=pdfium-mac-x64;     LIB=lib/libpdfium.dylib; SHA= ;;
  Darwin-arm64)  ASSET=pdfium-mac-arm64;   LIB=lib/libpdfium.dylib; SHA= ;;
  MINGW*|MSYS*|CYGWIN*) ASSET=pdfium-win-x64; LIB=bin/pdfium.dll;   SHA= ;;
  *) echo "no prebuilt pdfium known for $(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
curl -fsSL -o "$tmp/pdfium.tgz" \
  "https://github.com/bblanchon/pdfium-binaries/releases/download/${VERSION}/${ASSET}.tgz"

if [ -n "$SHA" ] && [ "$VERSION" = "chromium/7961" ]; then
  echo "$SHA  $tmp/pdfium.tgz" | sha256sum -c -
else
  echo "warning: no pinned checksum for ${ASSET} @ ${VERSION}; archive not verified" >&2
fi

mkdir -p "$tmp/x" "$DEST"
tar -xzf "$tmp/pdfium.tgz" -C "$tmp/x"
cp "$tmp/x/$LIB" "$DEST/"
cp "$tmp/x/LICENSE" "$DEST/LICENSE.pdfium"
echo "pdfium ${VERSION} (${ASSET}) -> ${DEST}"
