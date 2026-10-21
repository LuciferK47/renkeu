Real logs produced by Tectonic 0.16.9 (XeTeX engine) for the small projects
described below, captured with `keep_logs(true)` + `do_not_write_output_files()`.
They are golden inputs for `diagnostics.rs`; do not hand-edit them. To refresh,
re-run each project through the worker and copy `output.log`.

| fixture | project |
|---|---|
| undefined_cs | `main.tex` `\input{chapters/a}`; `chapters/a.tex` line 2 is `\undefinedmacro` |
| warnings | undefined `\ref`/`\cite`, an overfull `\hbox` |
| missing_sty | `\usepackage{nonexistentpkg}` |
| missing_input | `\input{nothere}` |
| missing_image | `\includegraphics{nope.png}` |
| missing_dollar | `x_1 and y^2` outside math |
| runaway | `\textbf{unclosed` |
| no_document | `\documentclass{article}` and nothing else |
| package_error | `\usepackage[nonsenseoption]{graphicx}` |
| latex_error_env | `\begin{itemize} ... \end{enumerate}` |
| bibtex (.log/.blg) | `\cite{good}\cite{ghost}`, `refs.bib` with a malformed entry |
