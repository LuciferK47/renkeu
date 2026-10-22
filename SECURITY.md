# Security

## Reporting a problem

Please do not open a public issue for a security problem. Use GitHub's private
vulnerability reporting on the repository that hosts the release you are using
(Security tab, "Report a vulnerability"). Say what you did, what you expected
and what happened; a project that reproduces it is the most useful thing you
can attach.

Renkeu is developed by one person alongside other work. Expect an answer within
a week, not within a day.

## What Renkeu is meant to protect

- **Your project and your key stay on your computer.** The only things sent
  out are listed in the app under Settings, Privacy.
- **The compile worker is a separate process with an empty `PATH` and no
  secrets in its environment**, and it is killed on timeout or cancel.
- **Paths from the browser, the AI and uploaded kits are confined to the
  project folder**: `..`, absolute paths, and symlinks that leave the folder
  are refused. AI edits are tried on a copy and shown to you before anything is
  written.

## What it is not

- **The local API has no authentication.** The desktop app binds it to a random
  loopback port and `latex-ai serve` binds `127.0.0.1` by default. Do not
  publish that port, and do not set `--host 0.0.0.0` without something in front
  that restricts access. Any program running as you can reach it.
- **The Windows installer is not code-signed yet**, so Windows SmartScreen warns
  when you first run it. Compare the installer's SHA-256 with `SHA256SUMS` on the
  release page.
- **Preflight checks are advice.** A clean submission report means nothing was
  found by text and PDF checks, not that a venue will accept the paper.

## Supported versions

Only the latest release.
