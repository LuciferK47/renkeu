import { describe, it, expect, vi } from 'vitest'

vi.mock('../src/lib/api/client.js', () => ({ api: { getFile: vi.fn(), saveFile: vi.fn() } }))

import { findBlock, emptyFields } from '../src/lib/templates.js'
import { DocumentStore } from '../src/lib/state/documents.svelte.js'

const fields = { title: 'A & B', authors: [{ name: 'Ada', affiliation: 'Co', email: '' }], abstract: 'Hi.\n\nBye.', keywords: 'x', date: '' }
const doc = (f = fields) =>
  `\\documentclass{article}\n% renkeu:frontmatter begin template=article fields=${JSON.stringify(f)}\n\\title{A \\& B}\n% renkeu:frontmatter end\n\\section{One}\n`

describe('findBlock', () => {
  it('reads the template and values and spans both marker lines', () => {
    const text = doc()
    const b = findBlock(text)
    expect(b.template).toBe('article')
    expect(b.fields.title).toBe('A & B')
    expect(b.fields.authors[0].name).toBe('Ada')
    expect(text.slice(b.start)).toMatch(/^% renkeu:frontmatter begin/)
    expect(text.slice(b.end)).toBe('\n\\section{One}\n')
  })

  it('fills in fields an older record lacks', () => {
    const b = findBlock(doc({ title: 'T' }))
    expect(b.fields).toEqual({ ...emptyFields(), title: 'T' })
  })

  it('is null for a hand-written document, a damaged record or a missing end', () => {
    expect(findBlock('\\documentclass{article}\n')).toBeNull()
    expect(findBlock('% renkeu:frontmatter begin template=article fields={oops\n% renkeu:frontmatter end')).toBeNull()
    expect(findBlock(doc().replace('% renkeu:frontmatter end', ''))).toBeNull()
  })
})

class Model {
  constructor(text) { this.text = text; this.calls = [] }
  getValue() { return this.text }
  getAlternativeVersionId() { return 1 }
  onDidChangeContent() { return { dispose() {} } }
  getPositionAt(offset) {
    const before = this.text.slice(0, offset).split('\n')
    return { lineNumber: before.length, column: before.at(-1).length + 1 }
  }
  pushEditOperations(_sel, edits) { this.calls.push(edits) }
}

describe('replaceRange', () => {
  it('turns offsets into a single editor edit', async () => {
    const { api } = await import('../src/lib/api/client.js')
    api.getFile.mockResolvedValue({ content: 'ab\ncdef\n', etag: 'e' })
    const model = new Model('ab\ncdef\n')
    const s = new DocumentStore({ create: async () => model, dispose() {} })
    await s.open('main.tex')
    expect(s.replaceRange('main.tex', 4, 6, 'XY')).toBe(true)
    expect(model.calls).toHaveLength(1)
    expect(model.calls[0][0]).toEqual({
      range: { startLineNumber: 2, startColumn: 2, endLineNumber: 2, endColumn: 4 },
      text: 'XY'
    })
  })

  it('does nothing for a file that is not open', () => {
    const s = new DocumentStore({ create: async () => new Model(''), dispose() {} })
    expect(s.replaceRange('nope.tex', 0, 1, 'x')).toBe(false)
  })
})
