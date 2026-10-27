#!/usr/bin/env bash
# Unpack a packaged CLI archive into an empty folder and use it the way the
# GitHub Action does: compile a document, count its pages, and measure it with
# the libpdfium that came in the archive. Runs on Linux and in Git Bash on
# Windows.
#
#   scripts/smoke-cli.sh dist/latex-ai-x86_64-unknown-linux-gnu.tar.gz
set -euo pipefail

archive="${1:?usage: smoke-cli.sh ARCHIVE}"
here="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

mkdir -p "$work/bin" "$work/doc" "$work/home"
tar -xzf "$archive" -C "$work/bin"
exe="$work/bin/latex-ai"
[ -f "$exe" ] || exe="$work/bin/latex-ai.exe"
[ -f "$exe" ] || { echo "FAIL: no latex-ai in the archive:"; ls "$work/bin"; exit 1; }

# Nothing outside the archive may supply pdfium, and the tool must not write
# into the user's real home.
unset LD_LIBRARY_PATH LATEX_AI_PDFIUM_DIR
export LATEX_AI_HOME="$work/home"

"$exe" doctor | tee "$work/doctor.txt"
grep -q "pdfium: ok" "$work/doctor.txt" || { echo "FAIL: doctor did not find the bundled libpdfium"; exit 1; }

cp "$here/examples/sample.tex" "$work/doc/main.tex"
# The first compile may download the TeX bundle, so be generous.
"$exe" check "$work/doc" --json > "$work/report.json"
pages="$(sed -nE 's/.*"pages": *([0-9]+).*/\1/p' "$work/report.json" | head -1)"
[ -n "$pages" ] && [ "$pages" -ge 1 ] || { echo "FAIL: no page count in the report:"; head -c 600 "$work/report.json"; exit 1; }
grep -q '"success": *true' "$work/report.json" || { echo "FAIL: the sample did not build"; head -c 600 "$work/report.json"; exit 1; }
grep -qi "not checked (libpdfium" "$work/report.json" && { echo "FAIL: libpdfium was not used"; exit 1; }

echo "OK: built the sample ($pages pages) with the packaged $(basename "$exe")"
