#!/usr/bin/env bash
# Runs `latex-ai check` for the Renkeu GitHub Action. All input arrives as
# INPUT_* environment variables, never spliced into the command line, so a
# value such as a branch-derived path cannot inject shell.
set -euo pipefail

bin="${INPUT_BINARY:-}"
if [ -z "$bin" ]; then
  version="${INPUT_VERSION:-latest}"
  repo="${INPUT_REPOSITORY:?no repository to download from}"
  asset="latex-ai-x86_64-unknown-linux-gnu.tar.gz"
  if [ "$version" = "latest" ]; then
    url="https://github.com/${repo}/releases/latest/download/${asset}"
  else
    url="https://github.com/${repo}/releases/download/${version}/${asset}"
  fi
  dir="${RUNNER_TEMP:-/tmp}/renkeu"
  mkdir -p "$dir"
  echo "Downloading ${url}" >&2
  curl -fsSL "$url" | tar -xz -C "$dir"
  bin="$dir/latex-ai"
fi
[ -x "$bin" ] || { echo "no executable latex-ai at: $bin" >&2; exit 2; }

args=("$bin" check "${INPUT_PATH:-.}" --github)
[ -z "${INPUT_MAIN:-}" ] || args+=(--main "$INPUT_MAIN")
[ -z "${INPUT_PAGES:-}" ] || args+=(--pages "$INPUT_PAGES")
[ -z "${INPUT_PROFILE:-}" ] || args+=(--profile "$INPUT_PROFILE")
[ "${INPUT_STRICT:-false}" != "true" ] || args+=(--strict)

exec "${args[@]}"
