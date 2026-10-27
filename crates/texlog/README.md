# texlog

Structured diagnostics from TeX and BibTeX logs.

A TeX log is not a grammar, it is a convention. `texlog` reads the conventions
that matter and returns errors, warnings and box problems with the file and line
they belong to, so a tool does not have to show users a 2 KB log.

- lines hard-wrapped at 79 columns are rejoined first;
- `(name` opens an input file and `)` closes it, which attributes each message to
  the right file;
- errors (`! message` with its `l.<n>` context line), `LaTeX Warning:` /
  `Package x Warning:` lines and `Overfull` / `Underfull` boxes are recognised;
- BibTeX `.blg` files (missing entries, undefined citations) are parsed too.

```rust
use texlog::{parse_tex_log, Severity};

let log = "(./main.tex\n! Undefined control sequence.\nl.3 The \\oops\n sum\n)";
let found = parse_tex_log(log, &|name| Some(name.trim_start_matches("./").to_owned()));
assert_eq!(found[0].severity, Severity::Error);
assert_eq!(found[0].line, Some(3));
```

It is a pure library: no I/O, no TeX engine, no `unsafe`. It was extracted from
[Renkeu](https://github.com/seriouslysahid/Latex-AI), where it feeds the problems
panel and the AI repair loop, and is tested against real logs (see
`tests/fixtures/logs`).

Licensed under either of Apache-2.0 or MIT at your option.
