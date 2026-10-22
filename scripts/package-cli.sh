#!/usr/bin/env bash
# Pack the `latex-ai` command-line tool with the libpdfium it needs into
# dist/latex-ai-<target>.tar.gz. The GitHub Action downloads this archive by
# name, so the name is part of the contract.
#
#   scripts/package-cli.sh x86_64-unknown-linux-gnu target/release/latex-ai
#   scripts/package-cli.sh x86_64-pc-windows-msvc   target/release/latex-ai.exe
#
# libpdfium comes from scripts/fetch-pdfium.sh (or PDFIUM_DIR). Without it the
# tool still compiles and counts pages, but cannot measure type size, fonts or
# render pages, so packaging refuses to go on without it.
set -euo pipefail

target="${1:?usage: package-cli.sh TARGET BINARY}"
binary="${2:?usage: package-cli.sh TARGET BINARY}"
here="$(cd "$(dirname "$0")/.." && pwd)"
out="${DIST_DIR:-$here/dist}"
pdfium="${PDFIUM_DIR:-$here/crates/latex_ai_tauri/resources/pdfium}"

case "$target" in
  *windows*) lib=pdfium.dll ;;
  *darwin*)  lib=libpdfium.dylib ;;
  *)         lib=libpdfium.so ;;
esac
[ -f "$binary" ] || { echo "no binary at $binary" >&2; exit 1; }
[ -f "$pdfium/$lib" ] || { echo "no $lib in $pdfium (run scripts/fetch-pdfium.sh)" >&2; exit 1; }

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
cp "$binary" "$stage/"
cp "$pdfium/$lib" "$stage/"
cp "$pdfium/LICENSE.pdfium" "$here/LICENSE-MIT" "$here/LICENSE-APACHE" "$stage/"

mkdir -p "$out"
archive="$out/latex-ai-$target.tar.gz"
tar -czf "$archive" -C "$stage" .
echo "$archive"
