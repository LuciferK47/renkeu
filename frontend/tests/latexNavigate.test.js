import { describe, it, expect, vi } from 'vitest'
import { referenceAt, definitionFor, hoverFor } from '../src/lib/latex/navigate.js'
import { proseMask, spellWords, misspellings, SpellChecker } from '../src/lib/latex/spell.js'
import { registerLatex } from '../src/lib/latex/register.js'
import { scan } from '../src/lib/latex/scan.js'

const ref = (line) => {
  const i = line.indexOf('|')
  return referenceAt(line.replace('|', ''), i + 1)
}

describe('referenceAt', () => {
  it('finds the key under the cursor in a comma list, not the whole group', () => {
    expect(ref('\\cite{aa, b|b ,cc}')).toMatchObject({ kind: 'cite', name: 'bb' })
    expect(ref('\\citep[see][p.~2]{a|a}')).toMatchObject({ kind: 'cite', name: 'aa' })
    expect(ref('\\cite{aa,|}')).toBeNull()
  })

  it('classifies refs, file arguments and bibliography styles', () => {
    expect(ref('see \\eqref{eq:|x}')).toMatchObject({ kind: 'ref', name: 'eq:x' })
    expect(ref('\\input{ch/o|ne}')).toMatchObject({ kind: 'include', name: 'ch/one' })
    expect(ref('\\includegraphics[width=1cm]{fi|g}')).toMatchObject({ kind: 'graphics', name: 'fig' })
    expect(ref('\\bibliography{a,b|}')).toMatchObject({ kind: 'bib', name: 'b', legacy: true })
    expect(ref('\\addbibresource{r|efs.bib}')).toMatchObject({ kind: 'bib', legacy: false })
  })

  it('falls back to the command under the cursor, and to nothing in plain text', () => {
    expect(ref('use \\fr|ac here')).toMatchObject({ kind: 'command', name: 'frac' })
    expect(ref('plain te|xt')).toBeNull()
    expect(ref('\\textbf{bo|ld}')).toBeNull()
  })
})

describe('definitions and hovers', () => {
  const snap = {
    labels: [{ name: 'sec:a', path: 'ch/one.tex', line: 7, text: '\\section{A}\\label{sec:a}' }],
    bib: [{ key: 'k84', type: 'article', path: 'refs.bib', line: 12, title: 'Literate Programming', author: 'Knuth', year: '1984' }],
    files: ['main.tex', 'ch/one.tex', 'fig/plot.png', 'refs.bib'],
    macroDefs: [{ name: 'R', path: 'main.tex', line: 3, text: '\\newcommand{\\R}{\\mathbb{R}}' }],
  }

  it('resolves labels, citations, macros and files the way TeX would', () => {
    expect(definitionFor({ kind: 'ref', name: 'sec:a' }, snap)).toEqual({ path: 'ch/one.tex', line: 7 })
    expect(definitionFor({ kind: 'cite', name: 'k84' }, snap)).toEqual({ path: 'refs.bib', line: 12 })
    expect(definitionFor({ kind: 'command', name: 'R' }, snap)).toEqual({ path: 'main.tex', line: 3 })
    expect(definitionFor({ kind: 'include', name: 'ch/one' }, snap)).toEqual({ path: 'ch/one.tex', line: 1 })
    expect(definitionFor({ kind: 'graphics', name: 'fig/plot' }, snap)).toEqual({ path: 'fig/plot.png', line: 1 })
    expect(definitionFor({ kind: 'bib', name: 'refs', legacy: true }, snap)).toEqual({ path: 'refs.bib', line: 1 })
    expect(definitionFor({ kind: 'bib', name: 'refs.bib', legacy: false }, snap)).toEqual({ path: 'refs.bib', line: 1 })
  })

  it('has no definition for what is not there', () => {
    for (const r of [{ kind: 'ref', name: 'nope' }, { kind: 'cite', name: 'nope' }, { kind: 'include', name: 'nope' }, { kind: 'command', name: 'frac' }]) {
      expect(definitionFor(r, snap)).toBeNull()
    }
    expect(definitionFor(null, snap)).toBeNull()
  })

  it('explains what a citation or label is, and warns when it is not defined', () => {
    expect(hoverFor({ kind: 'cite', name: 'k84' }, snap)).toContain('Literate Programming')
    expect(hoverFor({ kind: 'cite', name: 'k84' }, snap)).toContain('Knuth (1984)')
    expect(hoverFor({ kind: 'cite', name: 'zz' }, snap)).toMatch(/No bibliography entry named `zz`.*\[\?\]/)
    expect(hoverFor({ kind: 'ref', name: 'sec:a' }, snap)).toContain('ch/one.tex:7')
    expect(hoverFor({ kind: 'ref', name: 'zz' }, snap)).toMatch(/No \\label named `zz`.*\?\?/)
    expect(hoverFor({ kind: 'include', name: 'missing' }, snap)).toMatch(/not found/)
    expect(hoverFor({ kind: 'command', name: 'R' }, snap)).toContain('\\newcommand')
    expect(hoverFor({ kind: 'command', name: 'frac' }, snap)).toContain('Fraction')
    expect(hoverFor({ kind: 'command', name: 'zzz' }, snap)).toBeNull()
  })

  it('scan records where macros and labels live and the line they are on', () => {
    const r = scan('\\newcommand{\\R}{x}\n\\section{S}\\label{a} % c\n')
    expect(r.macroDefs).toEqual([{ name: 'R', line: 1, text: '\\newcommand{\\R}{x}' }])
    expect(r.labels).toEqual([{ name: 'a', line: 2, text: '\\section{S}\\label{a}' }])
  })
})

describe('spell: what counts as prose', () => {
  const words = (t) => spellWords(t).map((w) => w.word)

  it('skips comments, maths, command arguments and code', () => {
    const t = String.raw`Prose here % commentt
$mathx^2$ and \cite{keyy}, \ref{labell} \usepackage{pkgg}
\begin{equation} eqnword \end{equation}
\[ displayy \] \href{http://urll.com}{shown text} \label{lbl}`
    expect(words(t)).toEqual(['Prose', 'here', 'and', 'shown', 'text'])
  })

  it('skips accented words, acronyms, camelCase, short words and identifiers with digits', () => {
    expect(words(String.raw`Caf\'e na\"{i}ve NASA camelCase it x2 go ok`)).toEqual([])
  })

  it('keeps apostrophes inside words and splits hyphenated ones', () => {
    expect(words("don't well-known")).toEqual(["don't", 'well', 'known'])
  })

  it('reports 1-based line and column ranges', () => {
    expect(spellWords('ab\n  word here')[0]).toEqual({ word: 'word', line: 2, startColumn: 3, endColumn: 7 })
  })

  it('keeps every offset: masking never changes the length', () => {
    const t = '\\textbf{a} $x$ % c\n\\begin{verbatim}\nzz\n\\end{verbatim}\n'
    expect(proseMask(t)).toHaveLength(t.length)
  })

  it('reports only words the checker rejects', () => {
    const checker = { correct: (w) => w !== 'brwon' }
    expect(misspellings('quick brwon fox', checker).map((m) => m.word)).toEqual(['brwon'])
  })
})

describe('SpellChecker', () => {
  const speller = () => ({ correct: vi.fn((w) => w === 'good'), suggest: vi.fn(() => ['a', 'b', 'c', 'd', 'e', 'f']), add: vi.fn() })
  const store = (initial = []) => ({ data: initial, read() { return this.data }, write(w) { this.data = w } })

  it('accepts everything until the dictionary is loaded, so nothing is flagged early', () => {
    const c = new SpellChecker(async () => speller(), store())
    expect(c.ready).toBe(false)
    expect(c.correct('anything')).toBe(true)
  })

  it('loads once, applies the personal dictionary and limits suggestions', async () => {
    const s = speller()
    const load = vi.fn(async () => s)
    const c = new SpellChecker(load, store(['kushagra']))
    await Promise.all([c.ensureReady(), c.ensureReady()])
    expect(load).toHaveBeenCalledTimes(1)
    expect(s.add).toHaveBeenCalledWith('kushagra')
    expect(c.correct('good')).toBe(true)
    expect(c.correct('bad')).toBe(false)
    expect(c.correct('kushagra')).toBe(true)
    expect(c.suggest('bad')).toHaveLength(5)
  })

  it('adding a word persists it, tells listeners, and survives a failed load', async () => {
    const st = store()
    const c = new SpellChecker(async () => speller(), st)
    const seen = vi.fn()
    c.onChange(seen)
    c.add('zebra')
    expect(st.data).toEqual(['zebra'])
    expect(seen).toHaveBeenCalled()
    expect(c.correct('zebra')).toBe(true)

    const broken = new SpellChecker(async () => { throw new Error('offline') }, store())
    expect(await broken.ensureReady()).toBe(false)
    expect(broken.correct('whatever')).toBe(true)
  })
})

describe('registerLatex: hover and spelling quick fixes', () => {
  function monacoFake() {
    const p = {}
    return {
      p,
      editor: { registerCommand: vi.fn((id, fn) => (p[id] = fn)) },
      languages: {
        register() {}, setLanguageConfiguration() {}, setMonarchTokensProvider() {},
        registerCompletionItemProvider() {},
        registerHoverProvider: (l, h) => (p.hover = h),
        registerCodeActionProvider: (l, h) => (p.actions = h),
        CompletionItemKind: {}, CompletionItemInsertTextRule: {},
      },
    }
  }
  const snap = { labels: [{ name: 'x', path: 'a.tex', line: 2, text: '\\label{x}' }], bib: [], files: [], macros: [], macroDefs: [] }
  const index = { ensureLoaded: async () => {}, snapshot: () => snap }

  it('hover covers the reference name and says how to follow it', async () => {
    const m = monacoFake()
    registerLatex(m, () => ({ index }))
    const model = { getLineContent: () => 'see \\ref{x} ok' }
    const h = await m.p.hover.provideHover(model, { lineNumber: 4, column: 11 })
    expect(h.range).toEqual({ startLineNumber: 4, endLineNumber: 4, startColumn: 10, endColumn: 11 })
    expect(h.contents[0].value).toContain('a.tex:2')
    expect(h.contents[0].value).toContain('Ctrl+click')
    expect(await m.p.hover.provideHover({ getLineContent: () => 'plain' }, { lineNumber: 1, column: 2 })).toBeNull()
  })

  it('offers suggestions and "add to dictionary" for spelling markers only', () => {
    const m = monacoFake()
    const spell = { suggest: () => ['fix1', 'fix2'], add: vi.fn() }
    registerLatex(m, () => ({ index, spell }))
    const model = { uri: 'file:///a.tex', getVersionId: () => 3, getValueInRange: () => 'wrnog' }
    const spellMarker = { source: 'spell', startLineNumber: 1, endLineNumber: 1, startColumn: 1, endColumn: 6 }
    const other = { source: 'LaTeX', startLineNumber: 1, endLineNumber: 1, startColumn: 1, endColumn: 6 }

    const { actions } = m.p.actions.provideCodeActions(model, null, { markers: [spellMarker, other] })
    expect(actions.map((a) => a.title)).toEqual(['Change to “fix1”', 'Change to “fix2”', 'Add “wrnog” to the dictionary'])
    expect(actions[0].isPreferred).toBe(true)
    expect(actions[0].edit.edits[0].textEdit).toEqual({ range: { startLineNumber: 1, endLineNumber: 1, startColumn: 1, endColumn: 6 }, text: 'fix1' })

    m.p['latex-ai.spell.add']({}, 'wrnog')
    expect(spell.add).toHaveBeenCalledWith('wrnog')
  })

  it('offers the package for an undefined control sequence, and a citation search for an undefined citation', () => {
    const m = monacoFake()
    registerLatex(m, () => ({ index }))
    const text = '\\documentclass{article}\n\\begin{document}\n\\toprule\n\\end{document}\n'
    const model = {
      uri: 'file:///a.tex',
      getVersionId: () => 7,
      getValue: () => text,
      getLineContent: () => '\\toprule',
      getPositionAt: (offset) => ({ lineNumber: 2, column: offset - 24 + 1 })
    }
    const at = (message) => ({ source: 'LaTeX', message, startLineNumber: 3, endLineNumber: 3, startColumn: 1, endColumn: 8 })

    const fix = m.p.actions.provideCodeActions(model, null, { markers: [at('Undefined control sequence.')] }).actions
    expect(fix.map((a) => a.title)).toEqual(['Add \\usepackage{booktabs} (defines \\toprule)'])
    expect(fix[0].edit.edits[0].textEdit.text).toBe('\\usepackage{booktabs}\n')

    const cite = m.p.actions.provideCodeActions(model, null, { markers: [at("LaTeX Warning: Citation `x' on page 1 undefined on input line 3.")] }).actions
    expect(cite[0].command.arguments).toEqual(['citation'])

    expect(m.p.actions.provideCodeActions(model, null, { markers: [at('Missing $ inserted.')] }).actions).toEqual([])
  })
})
