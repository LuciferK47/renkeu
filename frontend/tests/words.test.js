import { describe, it, expect } from 'vitest'
import { countWords } from '../src/lib/latex/words.js'
import { fileGroup } from '../src/lib/files.js'

describe('countWords', () => {
  it('counts the words a reader sees, not the markup', () => {
    const src = [
      '\\documentclass{article}',
      '\\usepackage{amsmath}',
      '\\begin{document}',
      '\\section{Hello world}\\label{sec:x}',
      'This is \\textbf{very} good, see \\cite{knuth84} and Section~\\ref{sec:x}.',
      '% a comment with many words',
      'Inline $x^2 + y$ maths stays out. 50\\% done.',
      '\\begin{equation}',
      '  a = b + c',
      '\\end{equation}',
      '\\end{document}'
    ].join('\n')
    // Hello world / This is very good, see and Section / Inline maths stays out / 50 done
    expect(countWords(src)).toBe(2 + 7 + 4 + 2)
  })

  it('handles contractions, hyphens and non-English letters', () => {
    expect(countWords("don't well-known naïve")).toBe(3)
  })

  it('is zero for an empty or all-markup file', () => {
    expect(countWords('')).toBe(0)
    expect(countWords('\\begin{document}\n\\end{document}\n')).toBe(0)
  })
})

describe('fileGroup', () => {
  it('sorts files into the groups of the By type view', () => {
    expect(fileGroup('chapters/one.tex')).toBe('Documents')
    expect(fileGroup('a.cls')).toBe('Documents')
    expect(fileGroup('refs.BIB')).toBe('Bibliography')
    expect(fileGroup('fig/plot.png')).toBe('Figures')
    expect(fileGroup('fig/plot.pdf')).toBe('Figures')
    expect(fileGroup('notes.txt')).toBe('Other')
  })
})
