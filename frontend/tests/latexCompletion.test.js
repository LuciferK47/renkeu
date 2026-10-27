import { describe, it, expect, vi } from 'vitest'
import { contextAt, suggest, suggestBib } from '../src/lib/latex/completion.js'
import { registerLatex } from '../src/lib/latex/register.js'
import { ProjectIndex } from '../src/lib/latex/index.js'
import { markersFor } from '../src/lib/diagnostics.js'

const at = (line) => {
  const i = line.indexOf('|')
  return contextAt(line.replace('|', ''), i)
}

describe('contextAt', () => {
  it('knows citation commands, optional arguments and comma lists', () => {
    expect(at('see \\cite{kn|}')).toMatchObject({ kind: 'cite', prefix: 'kn', start: 10, closed: true })
    expect(at('\\citep[p.~3]{a,b, kn|')).toMatchObject({ kind: 'cite', prefix: 'kn', closed: false })
    expect(at('\\parencite{|')).toMatchObject({ kind: 'cite', prefix: '' })
    expect(at('\\citealp{x,|y}')).toMatchObject({ kind: 'cite', prefix: '' })
  })

  it('knows reference commands and distinguishes them from labels', () => {
    expect(at('\\eqref{eq:|')).toMatchObject({ kind: 'ref', prefix: 'eq:' })
    expect(at('\\cref{s|}')).toMatchObject({ kind: 'ref' })
    expect(at('\\label{s|}')).toBeNull()
  })

  it('knows file arguments', () => {
    expect(at('\\input{chap|')).toMatchObject({ kind: 'include', prefix: 'chap' })
    expect(at('\\includegraphics[width=2cm]{fi|')).toMatchObject({ kind: 'graphics', prefix: 'fi' })
    expect(at('\\bibliography{a,re|')).toMatchObject({ kind: 'bibfile', legacy: true, prefix: 're' })
    expect(at('\\addbibresource{re|')).toMatchObject({ kind: 'bibfile', legacy: false })
    expect(at('\\usepackage[utf8]{am|')).toMatchObject({ kind: 'package', prefix: 'am' })
  })

  it('distinguishes \\begin from \\end and takes over a lone closing brace', () => {
    expect(at('\\begin{ali|')).toMatchObject({ kind: 'env-begin', prefix: 'ali' })
    expect(at('\\end{ali|')).toMatchObject({ kind: 'env-end' })
    const withBrace = at('\\begin{|}')
    expect(withBrace.end).toBe(withBrace.start + 1)
  })

  it('completes a command from its backslash, and nothing in plain text', () => {
    expect(at('text \\fra|')).toMatchObject({ kind: 'command', prefix: 'fra', start: 5 })
    expect(at('\\|')).toMatchObject({ kind: 'command', prefix: '' })
    expect(at('plain wor|')).toBeNull()
    expect(at('a \\\\fr|')).toMatchObject({ kind: 'command' })
  })
})

describe('suggest', () => {
  const snap = {
    labels: [{ name: 'sec:a', path: 'main.tex', line: 4 }],
    bib: [{ key: 'knuth84', author: 'Knuth', year: '1984', title: 'Literate Programming', path: 'r.bib' }],
    files: ['main.tex', 'ch/one.tex', 'figs/a.png', 'figs/b.pdf', 'refs.bib'],
    macros: ['R'],
    environments: ['claim']
  }

  it('offers citation keys with author and year, closing the brace only if open', () => {
    const open = suggest(at('\\cite{k|'), snap)
    expect(open).toEqual([expect.objectContaining({ label: 'knuth84', insertText: 'knuth84}', detail: 'Knuth · 1984' })])
    expect(suggest(at('\\cite{k|}'), snap)[0].insertText).toBe('knuth84')
  })

  it('offers labels with where they are defined', () => {
    expect(suggest(at('\\ref{|'), snap)).toEqual([expect.objectContaining({ label: 'sec:a', detail: 'main.tex:4' })])
  })

  it('offers .tex files without extension, images with it, and .bib by command style', () => {
    expect(suggest(at('\\input{|'), snap).map((i) => i.label)).toEqual(['main', 'ch/one'])
    expect(suggest(at('\\includegraphics{|'), snap).map((i) => i.label)).toEqual(['figs/a.png', 'figs/b.pdf'])
    expect(suggest(at('\\bibliography{|'), snap).map((i) => i.label)).toEqual(['refs'])
    expect(suggest(at('\\addbibresource{|'), snap).map((i) => i.label)).toEqual(['refs.bib'])
  })

  it('expands \\begin{…} to a full block, and only names when the brace is elsewhere', () => {
    const block = suggest(at('\\begin{cl|'), snap).find((i) => i.label === 'claim')
    expect(block.insertText).toBe('claim}\n\t$0\n\\end{claim}')
    expect(block.snippet).toBe(true)
    const closedElsewhere = suggest(at('\\begin{cl|aim}'), snap).find((i) => i.label === 'claim')
    expect(closedElsewhere.insertText).toBe('claim')
  })

  it('includes project macros among the commands, and nothing for no context', () => {
    const labels = suggest(at('\\|'), snap).map((i) => i.label)
    expect(labels).toContain('\\R')
    expect(labels).toContain('\\frac')
    expect(suggest(null, snap)).toEqual([])
  })

  it('offers entry templates after @ in a .bib file only', () => {
    const { start, items } = suggestBib('  @ar')
    expect(start).toBe(2)
    expect(items.map((i) => i.label)).toContain('@article')
    expect(suggestBib('title = {x @ y}').items).toEqual([])
  })
})

describe('registerLatex', () => {
  function fakeMonaco() {
    const providers = {}
    return {
      providers,
      editor: { registerCommand: vi.fn((id, fn) => (providers[`cmd:${id}`] = fn)) },
      languages: {
        register: vi.fn(), setLanguageConfiguration: vi.fn(), setMonarchTokensProvider: vi.fn(),
        registerCompletionItemProvider: vi.fn((lang, p) => (providers[lang] = p)),
        registerHoverProvider: vi.fn((lang, p) => (providers[`${lang}-hover`] = p)),
        registerCodeActionProvider: vi.fn((lang, p) => (providers[`${lang}-actions`] = p)),
        CompletionItemKind: { Reference: 1, File: 2, Module: 3, Struct: 4, Snippet: 5, Function: 6 },
        CompletionItemInsertTextRule: { InsertAsSnippet: 4 },
      },
    }
  }
  const model = (line) => ({ getLineContent: () => line })
  const index = { ensureLoaded: vi.fn(async () => {}), snapshot: () => ({ labels: [{ name: 'x', path: 'a.tex', line: 1 }], bib: [], files: [], macros: [], environments: [] }) }

  it('registers both languages and completes labels at the cursor with an exact range', async () => {
    const m = fakeMonaco()
    registerLatex(m, () => ({ index }))
    expect(m.languages.register).toHaveBeenCalledWith(expect.objectContaining({ id: 'latex' }))
    expect(m.languages.register).toHaveBeenCalledWith(expect.objectContaining({ id: 'bibtex' }))

    const { suggestions } = await m.providers.latex.provideCompletionItems(model('\\ref{'), { lineNumber: 3, column: 6 })
    expect(suggestions).toHaveLength(1)
    expect(suggestions[0].label).toBe('x')
    expect(suggestions[0].range).toEqual({ startLineNumber: 3, endLineNumber: 3, startColumn: 6, endColumn: 6 })
  })

  it('completes nothing inside a comment or before the index exists', async () => {
    const m = fakeMonaco()
    registerLatex(m, () => ({ index }))
    expect((await m.providers.latex.provideCompletionItems(model('% \\ref{'), { lineNumber: 1, column: 8 })).suggestions).toEqual([])
    expect((await m.providers.latex.provideCompletionItems(model('50\\% \\ref{'), { lineNumber: 1, column: 11 })).suggestions).toHaveLength(1)

    const late = fakeMonaco()
    registerLatex(late, () => ({ index: null }))
    expect((await late.providers.latex.provideCompletionItems(model('\\ref{'), { lineNumber: 1, column: 6 })).suggestions).toEqual([])
  })
})

describe('ProjectIndex', () => {
  function setup({ open = {}, disk = {} } = {}) {
    const tree = Object.keys({ ...disk, ...open }).map((path) => ({ path, editable: true, kind: 'file' }))
    const project = { tree, editableFiles: () => tree, files: () => tree }
    const docs = Object.entries(open).map(([path, text]) => ({ path, text }))
    const documents = { docs, get: (p) => docs.find((d) => d.path === p) ?? null }
    const api = { getFile: vi.fn(async (p) => ({ content: disk[p] })) }
    return { index: new ProjectIndex({ project, documents, api }), api, docs, project }
  }

  it('merges open buffers (live) with files fetched once', async () => {
    const { index, api, docs } = setup({
      open: { 'main.tex': '\\label{live}' },
      disk: { 'refs.bib': '@article{k1, title={T}}', 'ch.tex': '\\label{fromdisk}\\newcommand{\\foo}{}' },
    })
    await index.ensureLoaded()
    const snap = index.snapshot()
    expect(snap.labels.map((l) => l.name).sort()).toEqual(['fromdisk', 'live'])
    expect(snap.bib.map((b) => b.key)).toEqual(['k1'])
    expect(snap.macros).toEqual(['foo'])
    expect(api.getFile).toHaveBeenCalledTimes(2)

    docs[0].text = '\\label{edited}'
    expect(index.snapshot().labels.map((l) => l.name)).toContain('edited')
    await index.ensureLoaded()
    expect(api.getFile).toHaveBeenCalledTimes(2) // cached
  })

  it('refetches after the file tree changes, and survives an unreadable file', async () => {
    const { index, api, project } = setup({ disk: { 'a.tex': '\\label{a}' } })
    await index.ensureLoaded()
    project.tree = [...project.tree]
    await index.ensureLoaded()
    expect(api.getFile).toHaveBeenCalledTimes(2)

    api.getFile.mockRejectedValue(new Error('gone'))
    index.invalidate()
    await expect(index.ensureLoaded()).resolves.toBeUndefined()
    expect(index.snapshot().labels).toEqual([])
  })
})

describe('markersFor', () => {
  const model = { getLineCount: () => 5, getLineMaxColumn: (n) => 10 + n }
  const sev = { Error: 8, Warning: 4 }

  it("keeps only this file's diagnostics, spans the line, and drops lines that no longer exist", () => {
    const found = {
      errors: [
        { file: 'a.tex', line: 2, message: 'Undefined control sequence.' },
        { file: 'b.tex', line: 2, message: 'other file' },
        { file: 'a.tex', line: 99, message: 'beyond the end' },
        { file: 'a.tex', line: null, message: 'no line' },
      ],
      warnings: [{ file: 'a.tex', line: 5, message: 'Overfull' }],
    }
    expect(markersFor('a.tex', found, model, sev)).toEqual([
      expect.objectContaining({ severity: 8, startLineNumber: 2, endColumn: 12, message: 'Undefined control sequence.' }),
      expect.objectContaining({ severity: 4, startLineNumber: 5, endColumn: 15 }),
    ])
  })
})
