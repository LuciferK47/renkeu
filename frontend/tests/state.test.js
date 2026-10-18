import { describe, it, expect, beforeEach, vi } from 'vitest'
import { APIError } from '../src/lib/api/client.js'
import { CompileState } from '../src/lib/state/compile.svelte.js'
import { ProjectState } from '../src/lib/state/project.svelte.js'
import { UiState } from '../src/lib/state/ui.svelte.js'

const diag = (severity, extra = {}) => ({ severity, kind: 'k', message: 'm', file: 'main.tex', line: 3, ...extra })

/** A compile whose SSE stream delivers the given events. */
function compileWith(events, api = {}) {
  const fullApi = { compile: vi.fn(async () => ({ job_id: 'j1', main_file: 'main.tex' })), cancelCompile: vi.fn(async () => ({})), ...api }
  const stream = vi.fn(async (_id, onEvent) => events.forEach(([event, data]) => onEvent({ event, data })))
  return { c: new CompileState({ api: fullApi, stream }), api: fullApi, stream }
}

describe('CompileState', () => {
  it('a failed compile lists located errors and remembers the build', async () => {
    const { c } = compileWith([
      ['progress', { progress: 0.5, message: 'Compiling with Tectonic...' }],
      ['compiled', { success: false, build_id: 'b'.repeat(32), main_file: 'main.tex', error_summary: 'x', diagnostics: [diag('error', { file: 'ch/a.tex', line: 2 }), diag('warning')] }]
    ])
    await c.start('ch/a.tex')

    expect(c.status).toBe('error')
    expect(c.errors).toEqual([expect.objectContaining({ file: 'ch/a.tex', line: 2 })])
    expect(c.warnings).toHaveLength(1)
    expect(c.buildId).toBe('b'.repeat(32))
    expect(c.mainFile).toBe('main.tex')
  })

  it('falls back to the summary when the server found no locations', async () => {
    const { c } = compileWith([['compiled', { success: false, error_summary: 'compile cancelled', diagnostics: [] }]])
    await c.start('main.tex')
    expect(c.errors).toEqual([{ message: 'compile cancelled' }])
  })

  it('success records pages and the build, clears errors', async () => {
    const { c } = compileWith([['compiled', { success: true, page_count: 4, build_id: 'a'.repeat(32), diagnostics: [diag('warning')] }]])
    await c.start('main.tex')
    expect(c.status).toBe('success')
    expect(c.pageCount).toBe(4)
    expect(c.errors).toEqual([])
    expect(c.warnings).toHaveLength(1)
    expect(c.lastCompileAt).toBeInstanceOf(Date)
  })

  it('an ambiguous project surfaces the candidates instead of a bare error', async () => {
    const err = new APIError('x', 409, { error: 'main_file_ambiguous', candidates: ['a.tex', 'b.tex'] })
    const { c } = compileWith([], { compile: vi.fn().mockRejectedValue(err) })
    await c.start('notes/n.tex')
    expect(c.status).toBe('error')
    expect(c.candidates).toEqual(['a.tex', 'b.tex'])
  })

  it('passes the chosen main file through', async () => {
    const { c, api } = compileWith([['compiled', { success: true }]])
    await c.start('ch.tex', { main: 'b.tex' })
    expect(api.compile).toHaveBeenCalledWith('ch.tex', expect.anything(), 'b.tex')
  })

  it('a request during a compile is coalesced into one rerun with the latest arguments', async () => {
    let finishFirst
    const api = { compile: vi.fn(async () => ({ job_id: 'j' })), cancelCompile: vi.fn() }
    const stream = vi.fn((_id, onEvent) => new Promise((resolve) => { finishFirst = () => { onEvent({ event: 'compiled', data: { success: true } }); resolve() } }))
    const c = new CompileState({ api, stream })

    const first = c.start('a.tex')
    await vi.waitFor(() => expect(stream).toHaveBeenCalledTimes(1))
    await c.start('b.tex')
    await c.start('c.tex') // replaces the queued request
    expect(api.compile).toHaveBeenCalledTimes(1)

    finishFirst()
    await vi.waitFor(() => expect(stream).toHaveBeenCalledTimes(2))
    finishFirst()
    await first
    expect(api.compile.mock.calls.map((x) => x[0])).toEqual(['a.tex', 'c.tex'])
  })

  it('cancel only acts on a running compile', async () => {
    const { c, api } = compileWith([])
    expect(await c.cancel()).toBe(false)
    c.status = 'compiling'
    c.jobId = 'job-9'
    expect(await c.cancel()).toBe(true)
    expect(api.cancelCompile).toHaveBeenCalledWith('job-9')
  })

  it('a network failure becomes an error state', async () => {
    const { c } = compileWith([], { compile: vi.fn().mockRejectedValue(new Error('fetch failed')) })
    await c.start('main.tex')
    expect(c.status).toBe('error')
    expect(c.errors).toEqual([{ message: 'fetch failed' }])
  })
})

describe('CompileState.restore', () => {
  const manifest = (over = {}) => ({ build_id: 'a'.repeat(32), main: 'main.tex', success: true, has_pdf: true, page_count: 4, diagnostics: [diag('warning')], ...over })

  it('shows the previous compile again without running one', async () => {
    const api = { getBuild: vi.fn(async () => manifest()), compile: vi.fn() }
    const c = new CompileState({ api })
    expect(await c.restore('a'.repeat(32))).toBe(true)
    expect(c.status).toBe('success')
    expect(c.pageCount).toBe(4)
    expect(c.buildId).toBe('a'.repeat(32))
    expect(c.mainFile).toBe('main.tex')
    expect(c.warnings).toHaveLength(1)
    expect(api.compile).not.toHaveBeenCalled()
  })

  it('restores a failed build as its errors', async () => {
    const c = new CompileState({ api: { getBuild: async () => manifest({ success: false, has_pdf: false, page_count: 0, diagnostics: [diag('error', { line: 9 })] }) } })
    await c.restore('a'.repeat(32))
    expect(c.status).toBe('error')
    expect(c.errors).toEqual([expect.objectContaining({ line: 9 })])
  })

  it('does nothing for a build that is gone, with no id, or once something has already run', async () => {
    expect(await new CompileState({ api: { getBuild: async () => { throw new Error('404') } } }).restore('a'.repeat(32))).toBe(false)
    expect(await new CompileState({ api: {} }).restore(null)).toBe(false)

    const busy = new CompileState({ api: { getBuild: vi.fn() } })
    busy.status = 'compiling'
    expect(await busy.restore('a'.repeat(32))).toBe(false)

    // A compile that starts while the request is in flight wins.
    const c = new CompileState({ api: { getBuild: async () => manifest() } })
    const pending = c.restore('a'.repeat(32))
    c.status = 'compiling'
    expect(await pending).toBe(false)
    expect(c.status).toBe('compiling')
  })
})

describe('ProjectState', () => {
  const tree = [
    { name: 'ch', path: 'ch', kind: 'dir', editable: false, children: [{ name: 'a.tex', path: 'ch/a.tex', kind: 'file', editable: true }, { name: 'p.png', path: 'ch/p.png', kind: 'file', editable: false }] },
    { name: 'main.tex', path: 'main.tex', kind: 'file', editable: true }
  ]

  beforeEach(() => localStorage.clear())

  it('flattens the tree and knows what is editable', async () => {
    const p = new ProjectState({ api: { getTree: async () => ({ tree }) } })
    await p.refresh()
    expect(p.files().map((f) => f.path)).toEqual(['ch/a.tex', 'ch/p.png', 'main.tex'])
    expect(p.editableFiles().map((f) => f.path)).toEqual(['ch/a.tex', 'main.tex'])
  })

  it('reveal opens every ancestor directory; toggle flips one', () => {
    const p = new ProjectState({ api: {} })
    p.reveal('a/b/c/file.tex')
    expect([...p.expanded].sort()).toEqual(['a', 'a/b', 'a/b/c'])
    p.toggle('a/b')
    expect(p.expanded.has('a/b')).toBe(false)
  })

  it('keeps the chosen root on the server and shows it after a reload', async () => {
    const saved = { main: null }
    const api = {
      getTree: async () => ({ tree }),
      getProject: async () => ({ main: saved.main }),
      setMain: vi.fn(async (main) => { saved.main = main; return { main } }),
    }
    const p = new ProjectState({ api })
    await p.setMain('main.tex')
    expect(api.setMain).toHaveBeenCalledWith('main.tex')

    const reloaded = new ProjectState({ api })
    await reloaded.refresh()
    expect(reloaded.mainOverride).toBe('main.tex')
  })

  it('drops a chosen root that no longer exists', async () => {
    const stale = new ProjectState({ api: { getTree: async () => ({ tree: [] }), getProject: async () => ({ main: 'gone.tex' }) } })
    await stale.refresh()
    expect(stale.mainOverride).toBeNull()
  })

  it('puts the previous choice back when the server refuses the new one', async () => {
    const p = new ProjectState({ api: { setMain: vi.fn().mockRejectedValue(new Error('not a .tex file')) } })
    p.mainOverride = 'a.tex'
    expect(await p.setMain('b.txt')).toBe(false)
    expect(p.mainOverride).toBe('a.tex')
    expect(p.error).toBe('not a .tex file')
  })

  it('a failed refresh keeps the old tree and reports the error', async () => {
    const p = new ProjectState({ api: { getTree: vi.fn().mockResolvedValueOnce({ tree }).mockRejectedValueOnce(new Error('down')) } })
    await p.refresh()
    await p.refresh()
    expect(p.error).toBe('down')
    expect(p.tree).toHaveLength(2)
  })
})

describe('UiState', () => {
  beforeEach(() => localStorage.clear())

  it('toggles and remembers panes across a reload', () => {
    const ui = new UiState()
    expect(ui.chat).toBe(true)
    ui.toggle('chat')
    ui.setSizes({ editor: 50 })
    const again = new UiState()
    expect(again.chat).toBe(false)
    expect(again.sizes.editor).toBe(50)
    expect(again.sizes.pdf).toBe(31)
  })

  it('ignores corrupt saved preferences', () => {
    localStorage.setItem('latex-ai:ui', '{"sidebar": "yes", "theme": "neon", "sizes": {"editor": "wide"}}')
    const ui = new UiState()
    expect(ui.sidebar).toBe(true)
    expect(ui.theme).toBe('auto')
    expect(ui.sizes.editor).toBe(32)
    localStorage.setItem('latex-ai:ui', 'not json')
    expect(new UiState().sidebar).toBe(true)
  })

  it('applies the theme to the document', () => {
    const ui = new UiState()
    ui.setTheme('dark')
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark')
    ui.setTheme('light')
    expect(document.documentElement.getAttribute('data-theme')).toBe('light')
  })

  it('toasts expire', () => {
    vi.useFakeTimers()
    const ui = new UiState()
    ui.toast('saved', 'success')
    expect(ui.toasts).toHaveLength(1)
    vi.advanceTimersByTime(4100)
    expect(ui.toasts).toHaveLength(0)
    vi.useRealTimers()
  })
})
