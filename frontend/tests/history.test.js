import { describe, it, expect, vi } from 'vitest'
import { restoreVersion, labelFor } from '../src/lib/history.js'

function setup({ detail, file, openDoc = null } = {}) {
  const api = {
    getHistoryEntry: vi.fn(async () => detail),
    getFile: vi.fn(async () => {
      if (file === null) throw Object.assign(new Error('not found'), { status: 404 })
      return file
    }),
    saveFile: vi.fn(async () => ({ etag: 'new' })),
    createFile: vi.fn(async () => ({ path: 'a.tex' })),
  }
  const documents = { get: vi.fn(() => openDoc), reload: vi.fn(async () => {}) }
  const project = { refresh: vi.fn(async () => {}) }
  const ui = { toasts: [], toast(message, type) { this.toasts.push({ message, type }) } }
  return { api, documents, project, ui, confirm: vi.fn(() => true) }
}
const entry = (over = {}) => ({ id: 7, file_path: 'a.tex', edit_type: 'manual-save', ...over })

describe('restoreVersion', () => {
  it('saves the entry text over the current file, conditionally on its etag', async () => {
    const d = setup({ detail: { text_after: 'old\n' }, file: { content: 'newer\n', etag: 'e1' } })
    expect(await restoreVersion(entry(), d)).toBe(true)
    expect(d.api.saveFile).toHaveBeenCalledWith('a.tex', 'old\n', 'e1')
    expect(d.project.refresh).toHaveBeenCalled()
    expect(d.ui.toasts.at(-1)).toMatchObject({ type: 'success' })
  })

  it('re-reads an open editor so it shows the restored text', async () => {
    const d = setup({ detail: { text_after: 'old\n' }, file: { content: 'x', etag: 'e' }, openDoc: { dirty: false } })
    await restoreVersion(entry(), d)
    expect(d.documents.reload).toHaveBeenCalledWith('a.tex')
  })

  it('asks before discarding unsaved edits, and stops if declined', async () => {
    const d = setup({ detail: { text_after: 'old' }, file: { content: 'x', etag: 'e' }, openDoc: { dirty: true } })
    d.confirm.mockReturnValue(false)
    expect(await restoreVersion(entry(), d)).toBe(false)
    expect(d.api.saveFile).not.toHaveBeenCalled()
  })

  it('brings a deleted file back from the text the delete kept', async () => {
    const d = setup({ detail: { text_after: '', text_before: 'precious\n' }, file: null })
    expect(await restoreVersion(entry({ edit_type: 'delete' }), d)).toBe(true)
    expect(d.api.createFile).toHaveBeenCalledWith('a.tex', 'precious\n')
    expect(d.api.saveFile).not.toHaveBeenCalled()
  })

  it('recreates a file that is gone even for an ordinary entry', async () => {
    const d = setup({ detail: { text_after: 'body' }, file: null })
    await restoreVersion(entry(), d)
    expect(d.api.createFile).toHaveBeenCalledWith('a.tex', 'body')
  })

  it('says so, and writes nothing, when the entry did not keep the text', async () => {
    const d = setup({ detail: { text_after: '', text_before: null }, file: null })
    expect(await restoreVersion(entry({ edit_type: 'delete' }), d)).toBe(false)
    expect(d.api.createFile).not.toHaveBeenCalled()
    expect(d.ui.toasts.at(-1).message).toMatch(/cannot be restored/)
  })

  it('does nothing when the file already has that content', async () => {
    const d = setup({ detail: { text_after: 'same' }, file: { content: 'same', etag: 'e' } })
    expect(await restoreVersion(entry(), d)).toBe(true)
    expect(d.api.saveFile).not.toHaveBeenCalled()
  })

  it('reports a conflict instead of overwriting a file that moved meanwhile', async () => {
    const d = setup({ detail: { text_after: 'old' }, file: { content: 'x', etag: 'e' } })
    d.api.saveFile.mockRejectedValue(Object.assign(new Error('etag mismatch'), { status: 409 }))
    expect(await restoreVersion(entry(), d)).toBe(false)
    expect(d.ui.toasts.at(-1)).toMatchObject({ type: 'error', message: expect.stringMatching(/changed while restoring/) })
  })
})

describe('undoing a change (restoring the version before it)', () => {
  it('writes the text the entry replaced', async () => {
    const d = setup({ detail: { text_after: 'new', text_before: 'old' }, file: { content: 'new', etag: 'e' } })
    expect(await restoreVersion(entry({ edit_type: 'apply-selected' }), { ...d, which: 'before' })).toBe(true)
    expect(d.api.saveFile).toHaveBeenCalledWith('a.tex', 'old', 'e')
    expect(d.ui.toasts.at(-1).message).toBe('Undid that change to a.tex')
  })

  it('says so, and writes nothing, for an old entry that kept no earlier text', async () => {
    const d = setup({ detail: { text_after: 'new', text_before: null }, file: { content: 'new', etag: 'e' } })
    expect(await restoreVersion(entry(), { ...d, which: 'before' })).toBe(false)
    expect(d.api.saveFile).not.toHaveBeenCalled()
    expect(d.ui.toasts.at(-1).message).toMatch(/cannot be undone/)
  })

  it('an ordinary restore still means "as it stood after this entry"', async () => {
    const d = setup({ detail: { text_after: 'after', text_before: 'before' }, file: { content: 'x', etag: 'e' } })
    await restoreVersion(entry(), d)
    expect(d.api.saveFile).toHaveBeenCalledWith('a.tex', 'after', 'e')
  })
})

describe('labelFor', () => {
  it('speaks plainly and passes unknown kinds through', () => {
    expect(labelFor('apply-selected')).toBe('AI change')
    expect(labelFor('agent-apply')).toBe('AI change')
    expect(labelFor('delete')).toBe('Deleted')
    expect(labelFor('something-new')).toBe('something-new')
  })
})
