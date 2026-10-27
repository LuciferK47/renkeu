import { describe, it, expect, vi } from 'vitest'
import { FileOps, isImage } from '../src/lib/files.js'

function setup({ docs = [], mainOverride = null, apiOver = {} } = {}) {
  const open = new Map(docs.map((p) => [p, { path: p }]))
  const documents = {
    get docs() { return [...open.values()] },
    activePath: docs[0] ?? null,
    get: (p) => open.get(p) ?? null,
    save: vi.fn(async () => true),
    close: vi.fn((p) => open.delete(p)),
    open: vi.fn(async (p) => { open.set(p, { path: p }) }),
    setActive: vi.fn(),
  }
  const project = {
    mainOverride,
    refresh: vi.fn(async () => {}),
    reveal: vi.fn(),
    setMain: vi.fn(async (m) => { project.mainOverride = m; return true }),
  }
  const ui = { toast: vi.fn() }
  const api = {
    createFile: vi.fn(async (path) => ({ path, etag: 'e' })),
    renameFile: vi.fn(async () => ({})),
    deleteFile: vi.fn(async () => null),
    fileEtag: vi.fn(async () => 'etag-1'),
    uploadFile: vi.fn(async () => ({})),
    ...apiOver,
  }
  return { ops: new FileOps({ documents, project, ui, api }), documents, project, ui, api }
}

describe('FileOps', () => {
  it('create makes the file, refreshes the tree and opens it', async () => {
    const { ops, api, documents, project } = setup()
    expect(await ops.create('ch/new.tex')).toBe('ch/new.tex')
    expect(api.createFile).toHaveBeenCalledWith('ch/new.tex')
    expect(project.refresh).toHaveBeenCalled()
    expect(project.reveal).toHaveBeenCalledWith('ch/new.tex')
    expect(documents.open).toHaveBeenCalledWith('ch/new.tex')
  })

  it('a refused create is reported and opens nothing', async () => {
    const { ops, ui, documents } = setup({ apiOver: { createFile: vi.fn().mockRejectedValue(new Error('file already exists')) } })
    expect(await ops.create('a.tex')).toBeNull()
    expect(ui.toast).toHaveBeenCalledWith(expect.stringContaining('already exists'), 'error')
    expect(documents.open).not.toHaveBeenCalled()
  })

  it('rename saves open tabs first, then reopens them under the new name', async () => {
    const { ops, documents, api } = setup({ docs: ['ch/a.tex', 'ch/b.tex', 'main.tex'] })
    expect(await ops.rename('ch', 'chapters')).toBe(true)

    expect(documents.save).toHaveBeenCalledWith('ch/a.tex')
    expect(documents.save).not.toHaveBeenCalledWith('main.tex')
    expect(api.renameFile).toHaveBeenCalledWith('ch', 'chapters')
    expect(documents.close).toHaveBeenCalledWith('ch/a.tex', { force: true })
    expect(documents.open).toHaveBeenCalledWith('chapters/a.tex')
    expect(documents.open).toHaveBeenCalledWith('chapters/b.tex')
    expect(documents.setActive).toHaveBeenCalledWith('chapters/a.tex')
  })

  it('a tab that cannot be saved stops the rename before anything moves', async () => {
    const { ops, documents, api, ui } = setup({ docs: ['a.tex'] })
    documents.save.mockResolvedValue(false)
    expect(await ops.rename('a.tex', 'b.tex')).toBe(false)
    expect(api.renameFile).not.toHaveBeenCalled()
    expect(documents.close).not.toHaveBeenCalled()
    expect(ui.toast).toHaveBeenCalled()
  })

  it('a refused rename leaves the tabs alone', async () => {
    const { ops, documents } = setup({ docs: ['a.tex'], apiOver: { renameFile: vi.fn().mockRejectedValue(new Error('b.tex already exists')) } })
    expect(await ops.rename('a.tex', 'b.tex')).toBe(false)
    expect(documents.close).not.toHaveBeenCalled()
  })

  it('renaming the main file moves the main-file choice with it', async () => {
    const { ops, project } = setup({ mainOverride: 'main.tex' })
    await ops.rename('main.tex', 'paper.tex')
    expect(project.setMain).toHaveBeenCalledWith('paper.tex')
  })

  it('deleting a folder deletes each file with the etag just read', async () => {
    const { ops, api, documents } = setup({ docs: ['ch/a.tex'] })
    const node = { kind: 'dir', path: 'ch', children: [
      { kind: 'file', path: 'ch/a.tex' },
      { kind: 'dir', path: 'ch/f', children: [{ kind: 'file', path: 'ch/f/p.png' }] },
    ] }
    expect(await ops.remove(node)).toBe(2)
    expect(api.deleteFile).toHaveBeenCalledWith('ch/a.tex', 'etag-1')
    expect(api.deleteFile).toHaveBeenCalledWith('ch/f/p.png', 'etag-1')
    expect(documents.close).toHaveBeenCalledWith('ch/a.tex', { force: true })
  })

  it('a file that changed under us is kept, and the failure is reported', async () => {
    const { ops, ui, project } = setup({
      mainOverride: 'a.tex',
      apiOver: { deleteFile: vi.fn().mockRejectedValue(Object.assign(new Error('etag mismatch'), { status: 409 })) },
    })
    expect(await ops.remove({ kind: 'file', path: 'a.tex' })).toBe(0)
    expect(ui.toast).toHaveBeenCalledWith(expect.stringContaining('a.tex'), 'error')
    expect(project.mainOverride).toBe('a.tex')
    expect(project.refresh).toHaveBeenCalled()
  })

  it('upload puts each file under the folder and counts the ones that worked', async () => {
    const good = new File(['x'], 'a.png')
    const bad = new File(['x'], 'run.sh')
    const { ops, api, ui } = setup({
      apiOver: { uploadFile: vi.fn(async (p) => { if (p.endsWith('.sh')) throw new Error('cannot be added'); return {} }) },
    })
    expect(await ops.upload([good, bad], 'figs')).toBe(1)
    expect(api.uploadFile).toHaveBeenCalledWith('figs/a.png', good)
    expect(ui.toast).toHaveBeenCalledWith(expect.stringContaining('run.sh'), 'error')
    expect(ui.toast).toHaveBeenCalledWith('Added 1 file', 'info')
  })
})

describe('isImage', () => {
  it('recognises what the preview can show', () => {
    expect(['a.png', 'b.JPG', 'c.jpeg', 'd.gif', 'e.svg'].every(isImage)).toBe(true)
    expect(['a.pdf', 'b.tex', 'png'].some(isImage)).toBe(false)
  })
})
