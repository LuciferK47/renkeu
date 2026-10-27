import { describe, it, expect, beforeEach, vi } from 'vitest'

vi.mock('../src/lib/api/client.js', () => ({
  api: { getFile: vi.fn(), saveFile: vi.fn() }
}))

import { api } from '../src/lib/api/client.js'
import { DocumentStore } from '../src/lib/state/documents.svelte.js'

/** A text model with the three things the store uses. */
class FakeModel {
  constructor(text) {
    this.text = text
    this.version = 1
    this.history = [{ v: 1, t: text }]
    this.listeners = []
  }
  getValue() { return this.text }
  getAlternativeVersionId() { return this.version }
  setValue(t) { this.edit(t) }
  onDidChangeContent(fn) { this.listeners.push(fn); return { dispose: () => (this.listeners = this.listeners.filter((l) => l !== fn)) } }
  /** Type: a new version. */
  edit(t) { this.text = t; this.version = Math.max(...this.history.map((h) => h.v)) + 1; this.history.push({ v: this.version, t }); this.listeners.forEach((l) => l()) }
  /** Undo to an earlier version: the version id goes back too, as in Monaco. */
  undoTo(v) { const h = this.history.find((x) => x.v === v); this.text = h.t; this.version = v; this.listeners.forEach((l) => l()) }
}

function store(opts) {
  const created = []
  const models = {
    create: async (path, text) => { const m = new FakeModel(text); created.push(m); return m },
    dispose: vi.fn()
  }
  return { s: new DocumentStore(models, { autosavePauseMs: 50, ...opts }), models, created }
}

const file = (content, etag) => ({ content, etag })

describe('opening', () => {
  beforeEach(() => vi.clearAllMocks())

  it('loads a file once, makes it active, and keeps one model per file', async () => {
    api.getFile.mockResolvedValue(file('hello', 'e1'))
    const { s, created } = store()
    const a = await s.open('a.tex')
    expect(s.activePath).toBe('a.tex')
    expect(a.etag).toBe('e1')
    expect(a.dirty).toBe(false)

    api.getFile.mockResolvedValue(file('other', 'e2'))
    await s.open('b.tex')
    await s.open('a.tex') // switching back must not reload or recreate
    expect(api.getFile).toHaveBeenCalledTimes(2)
    expect(created).toHaveLength(2)
    expect(s.activePath).toBe('a.tex')
  })

  it('opens nothing when the file cannot be read', async () => {
    api.getFile.mockRejectedValue(new Error('403'))
    const { s } = store()
    await expect(s.open('.env')).rejects.toThrow('403')
    expect(s.docs).toHaveLength(0)
    expect(s.activePath).toBeNull()
  })

  it('two opens of one file racing produce one tab', async () => {
    api.getFile.mockResolvedValue(file('x', 'e'))
    const { s } = store()
    await Promise.all([s.open('a.tex'), s.open('a.tex')])
    expect(s.docs).toHaveLength(1)
  })
})

describe('dirty tracking', () => {
  beforeEach(() => vi.clearAllMocks())

  it('typing makes it dirty and undoing back to the saved text makes it clean again', async () => {
    api.getFile.mockResolvedValue(file('v1', 'e1'))
    const { s } = store({ autosavePauseMs: 10_000 })
    const doc = await s.open('a.tex')
    const saved = doc.model.getAlternativeVersionId()

    doc.model.edit('v1!')
    expect(doc.dirty).toBe(true)
    doc.model.undoTo(saved)
    expect(doc.dirty).toBe(false)
  })
})

describe('saving', () => {
  beforeEach(() => vi.clearAllMocks())

  it('saves to its own path with its own ETag, and adopts the new one', async () => {
    api.getFile.mockResolvedValueOnce(file('a0', 'ea0')).mockResolvedValueOnce(file('b0', 'eb0'))
    api.saveFile.mockResolvedValue({ etag: 'ea1' })
    const { s } = store({ autosavePauseMs: 10_000 })
    const a = await s.open('a.tex')
    await s.open('b.tex') // b is now active
    a.model.edit('a1')

    expect(await s.save('a.tex')).toBe(true)

    // The save is for a.tex even though b.tex is the active tab.
    expect(api.saveFile).toHaveBeenCalledWith('a.tex', 'a1', 'ea0', { autosave: false })
    expect(a.etag).toBe('ea1')
    expect(a.dirty).toBe(false)
    expect(s.get('b.tex').etag).toBe('eb0')
  })

  it('text typed while a save is in flight stays dirty', async () => {
    api.getFile.mockResolvedValue(file('v0', 'e0'))
    let release
    api.saveFile.mockImplementation(() => new Promise((r) => (release = () => r({ etag: 'e1' }))))
    const { s } = store({ autosavePauseMs: 10_000 })
    const doc = await s.open('a.tex')
    doc.model.edit('v1')

    const saving = s.save('a.tex')
    await Promise.resolve()
    doc.model.edit('v2') // typed during the request
    release()

    expect(await saving).toBe(false)
    expect(doc.dirty).toBe(true)
    expect(doc.etag).toBe('e1')
  })

  it('saves for one document run in order, each with the previous ETag', async () => {
    api.getFile.mockResolvedValue(file('v0', 'e0'))
    const etags = []
    const releases = []
    api.saveFile.mockImplementation((_p, text, etag) => {
      etags.push(etag)
      return new Promise((r) => releases.push(() => r({ etag: `after-${text}` })))
    })
    const { s } = store({ autosavePauseMs: 10_000 })
    const doc = await s.open('a.tex')

    doc.model.edit('v1')
    const first = s.save('a.tex')
    await Promise.resolve()
    await Promise.resolve()
    doc.model.edit('v2') // typed while the first request is out
    const second = s.save('a.tex')

    // The second request must wait: sending it now would carry a stale ETag.
    expect(etags).toEqual(['e0'])
    releases[0]()
    await first
    await vi.waitFor(() => expect(etags).toEqual(['e0', 'after-v1']))
    releases[1]()
    await second

    expect(doc.dirty).toBe(false)
    expect(doc.etag).toBe('after-v2')
  })

  it('a save requested after the text is already saved does nothing', async () => {
    api.getFile.mockResolvedValue(file('v0', 'e0'))
    api.saveFile.mockResolvedValue({ etag: 'e1' })
    const { s } = store({ autosavePauseMs: 10_000 })
    const doc = await s.open('a.tex')
    doc.model.edit('v1')

    await Promise.all([s.save('a.tex'), s.save('a.tex'), s.save('a.tex')])

    expect(api.saveFile).toHaveBeenCalledTimes(1)
  })

  it('a 409 flags a conflict and keeps the text; it is not retried into a clobber', async () => {
    api.getFile.mockResolvedValue(file('v0', 'e0'))
    api.saveFile.mockRejectedValue(Object.assign(new Error('conflict'), { status: 409, payload: { current_etag: 'theirs' } }))
    const { s } = store({ autosavePauseMs: 10_000 })
    const doc = await s.open('a.tex')
    doc.model.edit('mine')

    expect(await s.save('a.tex')).toBe(false)
    expect(doc.conflict).toEqual({ serverEtag: 'theirs' })
    expect(doc.dirty).toBe(true)
    expect(doc.text).toBe('mine')

    // While in conflict, further saves do nothing at all.
    api.saveFile.mockClear()
    expect(await s.save('a.tex')).toBe(false)
    expect(api.saveFile).not.toHaveBeenCalled()
  })

  it('a network failure leaves it dirty with a message and no conflict', async () => {
    api.getFile.mockResolvedValue(file('v0', 'e0'))
    api.saveFile.mockRejectedValue(Object.assign(new Error('offline'), { status: 0 }))
    const { s } = store({ autosavePauseMs: 10_000 })
    const doc = await s.open('a.tex')
    doc.model.edit('mine')
    expect(await s.save('a.tex')).toBe(false)
    expect(doc.error).toBe('offline')
    expect(doc.conflict).toBeNull()
  })

  it('autosaves after a pause, marking the save as automatic, and not before', async () => {
    vi.useFakeTimers()
    api.getFile.mockResolvedValue(file('v0', 'e0'))
    api.saveFile.mockResolvedValue({ etag: 'e1' })
    const { s } = store({ autosavePauseMs: 100 })
    const doc = await s.open('a.tex')

    doc.model.edit('v1')
    await vi.advanceTimersByTimeAsync(60)
    doc.model.edit('v2') // still typing: the timer restarts
    await vi.advanceTimersByTimeAsync(60)
    expect(api.saveFile).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(60)
    expect(api.saveFile).toHaveBeenCalledWith('a.tex', 'v2', 'e0', { autosave: true })
    vi.useRealTimers()
  })

  it('does not autosave when switched off', async () => {
    vi.useFakeTimers()
    api.getFile.mockResolvedValue(file('v0', 'e0'))
    const { s } = store({ autosavePauseMs: 50 })
    s.autosave = false
    const doc = await s.open('a.tex')
    doc.model.edit('v1')
    await vi.advanceTimersByTimeAsync(500)
    expect(api.saveFile).not.toHaveBeenCalled()
    vi.useRealTimers()
  })

  it('flushAll saves every dirty document and says whether all are clean', async () => {
    api.getFile.mockResolvedValueOnce(file('a', 'ea')).mockResolvedValueOnce(file('b', 'eb'))
    api.saveFile.mockImplementation(async (p) => ({ etag: `${p}-new` }))
    const { s } = store({ autosavePauseMs: 10_000 })
    const a = await s.open('a.tex')
    const b = await s.open('b.tex')
    a.model.edit('a2')
    b.model.edit('b2')
    expect(await s.flushAll()).toBe(true)
    expect(s.anyDirty).toBe(false)

    a.model.edit('a3')
    api.saveFile.mockRejectedValue(Object.assign(new Error('x'), { status: 500 }))
    expect(await s.flushAll()).toBe(false)
  })
})

describe('external changes and conflicts', () => {
  beforeEach(() => vi.clearAllMocks())

  it('a clean document quietly follows the file on disk', async () => {
    api.getFile.mockResolvedValueOnce(file('old', 'e0'))
    const { s } = store()
    const doc = await s.open('a.tex')

    api.getFile.mockResolvedValue(file('new from disk', 'e1'))
    await s.checkExternalChanges()

    expect(doc.text).toBe('new from disk')
    expect(doc.etag).toBe('e1')
    expect(doc.dirty).toBe(false)
  })

  it('a dirty document is flagged, never overwritten', async () => {
    api.getFile.mockResolvedValueOnce(file('old', 'e0'))
    const { s } = store({ autosavePauseMs: 10_000 })
    const doc = await s.open('a.tex')
    doc.model.edit('my edit')

    api.getFile.mockResolvedValue(file('theirs', 'e1'))
    await s.checkExternalChanges()

    expect(doc.text).toBe('my edit')
    expect(doc.conflict).toEqual({ serverEtag: 'e1' })
  })

  it('an unchanged file and an unreachable server both leave things alone', async () => {
    api.getFile.mockResolvedValueOnce(file('same', 'e0'))
    const { s } = store()
    const doc = await s.open('a.tex')
    api.getFile.mockResolvedValue(file('same', 'e0'))
    await s.checkExternalChanges()
    api.getFile.mockRejectedValue(new Error('offline'))
    await s.checkExternalChanges()
    expect(doc.conflict).toBeNull()
    expect(doc.text).toBe('same')
  })

  it('"overwrite" keeps the local text and saves it against the server ETag', async () => {
    api.getFile.mockResolvedValueOnce(file('old', 'e0'))
    api.saveFile.mockRejectedValueOnce(Object.assign(new Error('c'), { status: 409, payload: { current_etag: 'theirs' } }))
    const { s } = store({ autosavePauseMs: 10_000 })
    const doc = await s.open('a.tex')
    doc.model.edit('mine')
    await s.save('a.tex')

    api.saveFile.mockResolvedValue({ etag: 'e2' })
    await s.resolveConflict('a.tex', 'overwrite')

    expect(api.saveFile).toHaveBeenLastCalledWith('a.tex', 'mine', 'theirs', { autosave: false })
    expect(doc.conflict).toBeNull()
    expect(doc.dirty).toBe(false)
  })

  it('"reload" takes the server text and drops the local edit', async () => {
    api.getFile.mockResolvedValueOnce(file('old', 'e0'))
    api.saveFile.mockRejectedValueOnce(Object.assign(new Error('c'), { status: 409, payload: { current_etag: 'theirs' } }))
    const { s } = store({ autosavePauseMs: 10_000 })
    const doc = await s.open('a.tex')
    doc.model.edit('mine')
    await s.save('a.tex')

    api.getFile.mockResolvedValue(file('server text', 'theirs'))
    await s.resolveConflict('a.tex', 'reload')

    expect(doc.text).toBe('server text')
    expect(doc.etag).toBe('theirs')
    expect(doc.dirty).toBe(false)
    expect(doc.conflict).toBeNull()
  })
})

describe('closing', () => {
  beforeEach(() => vi.clearAllMocks())

  it('refuses to close unsaved work unless forced, then activates a neighbour', async () => {
    api.getFile.mockResolvedValue(file('x', 'e'))
    const { s, models } = store({ autosavePauseMs: 10_000 })
    const a = await s.open('a.tex')
    await s.open('b.tex')
    await s.open('c.tex')
    s.setActive('b.tex')
    s.get('b.tex').model.edit('dirty')

    expect(s.close('b.tex')).toBe(false)
    expect(s.docs).toHaveLength(3)

    expect(s.close('b.tex', { force: true })).toBe(true)
    expect(models.dispose).toHaveBeenCalledTimes(1)
    expect(s.activePath).toBe('c.tex')
    expect(a.path).toBe('a.tex')

    s.close('c.tex')
    expect(s.activePath).toBe('a.tex')
    s.close('a.tex')
    expect(s.activePath).toBeNull()
  })

  it('closing cancels a pending autosave', async () => {
    vi.useFakeTimers()
    api.getFile.mockResolvedValue(file('x', 'e'))
    const { s } = store({ autosavePauseMs: 50 })
    const doc = await s.open('a.tex')
    doc.model.edit('y')
    s.close('a.tex', { force: true })
    await vi.advanceTimersByTimeAsync(200)
    expect(api.saveFile).not.toHaveBeenCalled()
    vi.useRealTimers()
  })
})
