# Renkeu check (GitHub Action)

Compiles your LaTeX project with Renkeu's engine (Tectonic, no TeX Live to
install) and tells you, on every push:

- whether it builds, with each error on the file and line it is about;
- how many pages the PDF has, and the first line that no longer fits when it
  is over your limit;
- overfull boxes, paragraphs that end in a word or two, and fonts that are
  not embedded.

The report goes to the job summary. The step fails when the document does not
build or has more pages than `pages`. Nothing in your repository is changed.

```yaml
- uses: actions/checkout@v4
- uses: seriouslysahid/Latex-AI/action@main
  with:
    path: paper        # a folder or a .tex file
    pages: 8           # fail above this
```

| Input | Default | |
|---|---|---|
| `path` | `.` | A `.tex` file, or a folder (its main file is found). |
| `main` | | The main file, when the folder has more than one candidate. |
| `pages` | | Fail when the PDF has more pages than this. |
| `strict` | `false` | Also fail on overfull boxes and unembedded fonts. |
| `binary` | | Path to a `latex-ai` binary. Otherwise `version` is downloaded from `repository`'s releases. |

Linux runners only. The step needs the same shared libraries as any
Tectonic build (`libfontconfig1`, `libgraphite2-3`, `libharfbuzz0b`,
`libicu`), which `ubuntu-latest` has.

Status: the release the default download points to does not exist yet (the
Linux and Windows release pipeline is milestone M23). Until it does, build
`latex-ai` in an earlier step and pass it as `binary`.
