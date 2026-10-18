import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte'

const editors = []
vi.mock('../src/lib/monaco.js', () => {
  const fake = () => {
    const ed = {
      model: null,
      setModel: vi.fn((m) => (ed.model = m)),
      saveViewState: vi.fn(() => ({ saved: ed.model?.name })),
      restoreViewState: vi.fn(),
      focus: vi.fn(),
      dispose: vi.fn(),
      onDidChangeCursorPosition: vi.fn((cb) => (ed.cursor = cb)),
      revealLineInCenter: vi.fn(),
      setPosition: vi.fn(),
      getModel: vi.fn(() => ed.model),
      getPosition: vi.fn(() => ({ lineNumber: 1, column: 8 })),
      onMouseDown: vi.fn((cb) => (ed.mouseDown = cb)),
      updateOptions: vi.fn(),
    }
    editors.push(ed)
    return ed
  }
  const monaco = { editor: { create: vi.fn(fake), setTheme: vi.fn(), setModelMarkers: vi.fn() }, MarkerSeverity: { Error: 8, Warning: 4, Info: 2 } }
  return { loadMonaco: vi.fn(async () => monaco), __monaco: monaco }
})

import { __monaco } from '../src/lib/monaco.js'
import EditorPane from '../src/components/EditorPane.svelte'
import { DocumentStore } from '../src/lib/state/documents.svelte.js'
import { CommandRegistry } from '../src/lib/commands.js'
import { UiState } from '../src/lib/state/ui.svelte.js'
import { CompileState } from '../src/lib/state/compile.svelte.js'

vi.mock('../src/lib/api/client.js', () => ({
  api: { getFile: vi.fn(async (path) => ({ content: `text of ${path}`, etag: `etag-${path}` })), saveFile: vi.fn() },
}))

class Model {
  constructor(name, text) { this.name = name; this.text = text }
  getLineCount() { return 10 }
  getLineContent() { return this.line ?? 'see \\ref{lab} here' }
  getValueInRange() { return '' }
  getLineMaxColumn() { return 20 }
  getValue() { return this.text }
  setValue(t) { this.text = t }
  getAlternativeVersionId() { return 1 }
  onDidChangeContent() { return { dispose() {} } }
}

function setup(extra = {}) {
  localStorage.clear()
  editors.length = 0
  const documents = new DocumentStore({ create: async (p, t) => new Model(p, t), dispose() {} })
  const commands = new CommandRegistry()
  const ui = new UiState()
  const onclosetab = vi.fn()
  const oncursor = vi.fn()
  const view = render(EditorPane, { props: { documents, ui, commands, onclosetab, oncursor, ...extra } })
  return { documents, commands, ui, onclosetab, oncursor, component: view.component, ...view }
}

describe('EditorPane', () => {
  beforeEach(() => vi.clearAllMocks())

  // The effect must subscribe to the diagnostics even when it first runs before
  // the editor exists; a compile that fails afterwards must still be drawn.
  it('Ctrl+click and F12 follow the reference under the cursor, with the definition when there is one', async () => {
    const index = { ensureLoaded: vi.fn(async () => {}), snapshot: () => ({ labels: [{ name: 'lab', path: 'ch.tex', line: 9, text: '' }], bib: [], files: [], macroDefs: [] }) }
    const onfollow = vi.fn()
    const { documents, component } = setup({ index, onfollow })
    await documents.open('a.tex')
    await waitFor(() => expect(editors[0]?.model?.name).toBe('a.tex'))

    editors[0].mouseDown({ event: { ctrlKey: true, leftButton: true, preventDefault() {} }, target: { position: { lineNumber: 1, column: 10 } } })
    await waitFor(() => expect(onfollow).toHaveBeenCalledWith({ ref: expect.objectContaining({ kind: 'ref', name: 'lab' }), def: { path: 'ch.tex', line: 9 } }))

    // A plain click does nothing.
    onfollow.mockClear()
    editors[0].mouseDown({ event: { ctrlKey: false, leftButton: true, preventDefault() {} }, target: { position: { lineNumber: 1, column: 10 } } })
    expect(onfollow).not.toHaveBeenCalled()

    expect(await component.followReference()).toBe(true)
    expect(onfollow).toHaveBeenCalledTimes(1)
  })

  it('checks spelling of prose files once the dictionary is ready, and clears when switched off', async () => {
    const spell = { ensureReady: vi.fn(async () => true), onChange: vi.fn(() => () => {}), correct: (w) => w !== 'wrnog', suggest: () => [] }
    const { documents, ui } = setup({ spell })
    await documents.open('a.tex')
    await documents.open('refs.bib')
    documents.get('a.tex').model.getValue = () => 'a wrnog word'
    documents.get('a.tex').model.onDidChangeContent = vi.fn(() => ({ dispose() {} }))
    documents.get('refs.bib').model.onDidChangeContent = vi.fn(() => ({ dispose() {} }))
    ui.spellcheck = false
    ui.spellcheck = true

    const spellCalls = () => __monaco.editor.setModelMarkers.mock.calls.filter((c) => c[1] === 'latex-spell')
    await waitFor(() => expect(spellCalls().some((c) => c[2].length === 1)).toBe(true))
    const flagged = spellCalls().find((c) => c[2].length === 1)
    expect(flagged[2][0]).toMatchObject({ source: 'spell', severity: 2, startColumn: 3, endColumn: 8, message: 'Unknown word: wrnog' })
    // BibTeX is not checked as prose.
    expect(spellCalls().some((c) => c[0] === documents.get('refs.bib').model && c[2].length > 0)).toBe(false)

    ui.spellcheck = false
    await waitFor(() => expect(spellCalls().at(-1)[2]).toEqual([]))
  })

  it('applies the editor preferences from Settings, now and when they change', async () => {
    const { documents, ui } = setup()
    await documents.open('a.tex')
    await waitFor(() => expect(editors).toHaveLength(1))
    await waitFor(() => expect(editors[0].updateOptions).toHaveBeenCalledWith({ fontSize: 14, tabSize: 4, wordWrap: 'on' }))

    ui.setEditor({ fontSize: 20, tabSize: 2, wordWrap: false })
    await waitFor(() => expect(editors[0].updateOptions).toHaveBeenLastCalledWith({ fontSize: 20, tabSize: 2, wordWrap: 'off' }))
  })

  it('draws compile errors as markers on the files they name, including after the editor loads', async () => {
    const compile = new CompileState({ api: {} })
    const { documents } = setup({ compile })
    await documents.open('a.tex')
    await waitFor(() => expect(editors).toHaveLength(1))

    compile.errors = [{ file: 'a.tex', line: 3, message: 'Undefined control sequence.' }]
    await waitFor(() =>
      expect(__monaco.editor.setModelMarkers).toHaveBeenLastCalledWith(
        documents.get('a.tex').model,
        'latex-compile',
        [expect.objectContaining({ severity: 8, startLineNumber: 3, message: 'Undefined control sequence.' })],
      ),
    )

    compile.errors = []
    await waitFor(() =>
      expect(__monaco.editor.setModelMarkers).toHaveBeenLastCalledWith(documents.get('a.tex').model, 'latex-compile', []),
    )
  })

  it('invites you to open a file when none is open', () => {
    setup()
    expect(screen.getByText('No file open')).toBeTruthy()
  })

  it('creates one editor and shows the active document\'s own model', async () => {
    const { documents } = setup()
    await documents.open('a.tex')
    await documents.open('b.tex')
    await waitFor(() => expect(editors).toHaveLength(1))
    await waitFor(() => expect(editors[0].model?.name).toBe('b.tex'))

    documents.setActive('a.tex')
    await waitFor(() => expect(editors[0].model?.name).toBe('a.tex'))
    expect(__monaco.editor.create).toHaveBeenCalledTimes(1)
  })

  it('remembers the cursor and scroll of the tab you leave and restores them on return', async () => {
    const { documents } = setup()
    await documents.open('a.tex')
    await documents.open('b.tex')
    await waitFor(() => expect(editors[0]?.model?.name).toBe('b.tex'))

    documents.setActive('a.tex') // leaving b
    await waitFor(() => expect(editors[0].model.name).toBe('a.tex'))
    documents.setActive('b.tex') // back to b
    await waitFor(() => expect(editors[0].model.name).toBe('b.tex'))

    expect(editors[0].restoreViewState).toHaveBeenCalledWith({ saved: 'b.tex' })
  })

  it('shows a tab per document, selects on click, and asks to close', async () => {
    const { documents, onclosetab } = setup()
    await documents.open('chapters/a.tex')
    await documents.open('b.tex')
    await waitFor(() => expect(screen.getAllByRole('tab')).toHaveLength(2))

    expect(screen.getByRole('tab', { name: 'b.tex' }).getAttribute('aria-selected')).toBe('true')
    await fireEvent.click(screen.getByRole('tab', { name: 'a.tex' }))
    expect(documents.activePath).toBe('chapters/a.tex')

    await fireEvent.click(screen.getByRole('button', { name: 'Close chapters/a.tex' }))
    expect(onclosetab).toHaveBeenCalledWith('chapters/a.tex')
  })

  it('marks unsaved and conflicted tabs', async () => {
    const { documents } = setup()
    const a = await documents.open('a.tex')
    const b = await documents.open('b.tex')
    a.dirty = true
    b.conflict = { serverEtag: 'x' }
    await waitFor(() => expect(screen.getByLabelText('unsaved changes')).toBeTruthy())
    expect(screen.getByLabelText('conflict')).toBeTruthy()
  })

  it('reveals a line and reports the cursor', async () => {
    const { documents, oncursor, component } = setup()
    await documents.open('a.tex')
    await waitFor(() => expect(editors).toHaveLength(1))

    component.reveal(7, 3)
    expect(editors[0].revealLineInCenter).toHaveBeenCalledWith(7)
    expect(editors[0].setPosition).toHaveBeenCalledWith({ lineNumber: 7, column: 3 })

    editors[0].cursor({ position: { lineNumber: 4, column: 9 } })
    expect(oncursor).toHaveBeenCalledWith({ line: 4, column: 9 })
  })

  it('lets the command registry see shortcuts before the editor does', async () => {
    const { documents, commands } = setup()
    const run = vi.fn()
    commands.register({ id: 'compile', title: 'Compile', keys: 'Ctrl+Enter', run })
    await documents.open('a.tex')
    await waitFor(() => expect(editors).toHaveLength(1))

    const host = document.querySelector('.host')
    const reachedEditor = vi.fn()
    host.addEventListener('keydown', reachedEditor)
    const inner = document.createElement('textarea')
    host.append(inner)
    const event = new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true, cancelable: true })
    inner.dispatchEvent(event)

    expect(run).toHaveBeenCalled()
    expect(event.defaultPrevented).toBe(true)
    expect(reachedEditor).not.toHaveBeenCalled()
  })

  it('follows the theme', async () => {
    const { documents, ui } = setup()
    await documents.open('a.tex')
    await waitFor(() => expect(editors).toHaveLength(1))
    ui.setTheme('dark')
    await waitFor(() => expect(__monaco.editor.setTheme).toHaveBeenLastCalledWith('vs-dark'))
    ui.setTheme('light')
    await waitFor(() => expect(__monaco.editor.setTheme).toHaveBeenLastCalledWith('vs'))
  })

  it('disposes the editor when it goes away', async () => {
    const { documents, unmount } = setup()
    await documents.open('a.tex')
    await waitFor(() => expect(editors).toHaveLength(1))
    unmount()
    expect(editors[0].dispose).toHaveBeenCalled()
  })
})
