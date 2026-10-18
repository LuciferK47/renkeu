import { describe, it, expect } from 'vitest'
import { scan, stripComment, balancedGroup } from '../src/lib/latex/scan.js'
import { scanBib } from '../src/lib/latex/bibtex.js'

describe('stripComment', () => {
  it('cuts at an unescaped %, keeps an escaped one', () => {
    expect(stripComment('a % b')).toBe('a ')
    expect(stripComment('50\\% off')).toBe('50\\% off')
    expect(stripComment('end of line \\\\% comment')).toBe('end of line \\\\')
    expect(stripComment('no comment')).toBe('no comment')
  })
})

describe('balancedGroup', () => {
  it('reads nested braces and skips escaped ones', () => {
    expect(balancedGroup('{a {b} \\} c}x', 0)).toBe('a {b} \\} c')
    expect(balancedGroup('{open', 0)).toBeNull()
    expect(balancedGroup('x{a}', 0)).toBeNull()
  })
})

describe('scan', () => {
  const src = String.raw`\documentclass{article}
% !TEX root = ../main.tex
\usepackage[utf8]{inputenc}
\usepackage{amsmath, graphicx}
\newcommand{\R}{\mathbb{R}}
\def\eps{\varepsilon}
\newenvironment{claim}{}{}
\begin{document}
\section{Intro \label{sec:intro}}
Text \label{eq:a} and % \label{eq:commented}
\subsection*{Details with $x^{2}$}
\chapter[short]{Long {nested} title}
\input{chapters/one}
\include{two} \includegraphics[width=3cm]{figs/a.png}
\bibliography{refs, more}
\addbibresource{x.bib}
\begin{verbatim}
\section{Not a heading}
\label{not-a-label}
\end{verbatim}
\section{After}
\end{document}`

  const r = scan(src)

  it('finds headings with levels, nesting and star', () => {
    expect(r.sections.map((s) => [s.level, s.title, s.line, s.starred])).toEqual([
      [2, 'Intro', 9, false],
      [3, 'Details with $x^{2}$', 11, true],
      [1, 'Long {nested} title', 12, false],
      [2, 'After', 21, false],
    ])
  })

  it('ignores commented-out and verbatim content', () => {
    expect(r.labels.map((l) => l.name)).toEqual(['sec:intro', 'eq:a'])
  })

  it('finds the files the document pulls in', () => {
    expect(r.includes.map((i) => [i.kind, i.target])).toEqual([
      ['input', 'chapters/one'],
      ['include', 'two'],
      ['graphics', 'figs/a.png'],
      ['bib', 'refs.bib'],
      ['bib', 'more.bib'],
      ['bib', 'x.bib'],
    ])
  })

  it('collects packages, macros, environments and the root magic comment', () => {
    expect(r.packages).toEqual(['inputenc', 'amsmath', 'graphicx'])
    expect(r.macros).toEqual(['R', 'eps'])
    expect(r.environments).toEqual(['claim'])
    expect(r.root).toBe('../main.tex')
  })

  it('copes with CRLF and an empty file', () => {
    expect(scan('\\section{A}\r\n\\label{x}\r\n').labels).toEqual([{ name: 'x', line: 2 }])
    expect(scan('').sections).toEqual([])
  })
})

describe('scanBib', () => {
  const bib = `@string{ieee = "IEEE"}
@comment{ignored}
@article{knuth84,
  author = {Donald E. Knuth},
  title = {Literate {P}rogramming},
  year = 1984,
}

@Book{lamport94, author = "Leslie Lamport", title="LaTeX: A Document Preparation System",
  date = {1994-01-01}}
@preamble{"\\newcommand{\\x}{y}"}`

  it('lists real entries with their display fields, skipping @string/@comment/@preamble', () => {
    const entries = scanBib(bib)
    expect(entries.map((e) => [e.key, e.type, e.line])).toEqual([['knuth84', 'article', 3], ['lamport94', 'book', 9]])
    expect(entries[0]).toMatchObject({ author: 'Donald E. Knuth', title: 'Literate Programming', year: '1984' })
    expect(entries[1]).toMatchObject({ author: 'Leslie Lamport', title: 'LaTeX: A Document Preparation System', year: '1994' })
  })
})
