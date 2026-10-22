# From alpha to public beta: what has to be true, and what to post

This is a checklist for the owner. Nothing here has been done; each line says
what has to be true first.

## Before the first alpha build goes to anyone

- [ ] The release workflow has run on a test tag and its Windows jobs are green
      (they have never run). Remove `continue-on-error` from the `windows` CI job.
- [ ] A public releases repository exists (`seriouslysahid/renkeu-releases` is
      the name the GitHub Action and the feedback link use), with Issues enabled.
      Set the repository variable `RELEASE_REPO` and the secret `RELEASES_TOKEN`
      here.
- [ ] Install the Windows installer and the AppImage on machines you do not
      develop on. The Windows window launch is not covered by any test.
- [ ] Read `docs/alpha.md` as a stranger would. Replace the file names with the
      real ones.

## Alpha (10–20 people)

- Who: BITS labs, friends' research groups. Ask each for one real paper and its
  page limit, not a toy.
- The one task is in `docs/alpha.md`. Count for each tester: did the build match
  their usual one, did fit reach the limit, and did they accept the result.
- Fix what repeats across three testers before adding anything.

## Before the public beta

- [ ] Windows and Linux installers install, start, compile and render on clean
      machines.
- [ ] The site's "Get Renkeu" goes to real downloads, and the landing copy test
      still passes. The "View source on GitHub" button points at a repository that
      is private; either make the source public, or change the button.
- [ ] The first-run path works without an API key: today the assistant needs a
      key even for a local Ollama model, which will confuse new users.
- [ ] A trademark and domain search for "Renkeu".
- [ ] The Tauri updater, which needs a signing key only you can create.

## Signals to watch (no telemetry)

Release download counts, Action and MCP installs (GitHub code search for
`uses: seriouslysahid/Latex-AI/action`), crate downloads if the crates are
published, and the issues and replies you get.

## Where to post, and what each post must say

Write each post after the beta is installable. Say what it does, what it does
not, and that the AI features need a key or a local model. Do not quote a number
you have not measured; the page-fit benchmark in `bench/fit` has so far been run
only on a small CPU-bound local model and no paper reached its limit, so it
cannot support a claim about how well fit works.

| Where | What fits there |
|---|---|
| Show HN | The single binary with an embedded engine, the CLI and MCP, and the privacy boundary. Link the repository only when it is public. |
| r/LaTeX | The page-count and overfull report, SyncTeX both ways, submission profiles. |
| r/rust | How the compile worker, SyncTeX reader and pdfium fit together; what Tectonic does and does not do. |
| This Week in Rust | A crate or the project, once something is published and public. |
| TUG mailing list | The SyncTeX reader and the log parser, framed as tools for the TeX community. |
