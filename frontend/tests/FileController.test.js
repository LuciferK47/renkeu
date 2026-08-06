import { describe, it, expect, beforeEach, vi } from 'vitest'
import { get } from 'svelte/store'

vi.mock('../src/services/fileService.js', () => ({
  loadFile: vi.fn(),
  saveFile: vi.fn(),
  listFiles: vi.fn(),
  searchWorkspace: vi.fn(),
}))

import * as fileService from '../src/services/fileService.js'
import { loadFile, saveFile } from '../src/controllers/FileController.js'
import { fileStore } from '../src/stores/fileStore.js'
import { uiStore } from '../src/stores/uiStore.js'

describe('FileController', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    fileStore.set({
      filePath: null,
      content: '',
      etag: null,
      dirty: false,
      lastSavedAt: null,
      savedContent: '',
    })
    uiStore.update((ui) => ({ ...ui, status: '', toasts: [] }))
  })

  describe('loadFile', () => {
    it('puts the file in the buffer and marks it clean', async () => {
      fileService.loadFile.mockResolvedValue({ content: '\\documentclass{article}', eTag: 'abc' })
      await loadFile('main.tex')

      const f = get(fileStore)
      expect(f.filePath).toBe('main.tex')
      expect(f.content).toBe('\\documentclass{article}')
      expect(f.etag).toBe('abc')
      expect(f.dirty).toBe(false)
      // savedContent is the baseline dirty-tracking compares against; if it
      // were left empty the file would look modified the moment it opened.
      expect(f.savedContent).toBe('\\documentclass{article}')
    })

    it('does nothing at all without a path', async () => {
      await loadFile('')
      expect(fileService.loadFile).not.toHaveBeenCalled()
    })

    it('reports a failure instead of leaving a half-loaded buffer', async () => {
      fileService.loadFile.mockRejectedValue(new Error('not found'))
      await loadFile('missing.tex')

      expect(get(uiStore).status).toContain('not found')
      expect(get(fileStore).content).toBe('')
    })
  })

  describe('saveFile', () => {
    it('records the new etag and clears dirty', async () => {
      fileService.saveFile.mockResolvedValue({ etag: 'v2' })
      fileStore.update((f) => ({ ...f, filePath: 'main.tex', dirty: true }))

      const res = await saveFile('main.tex', 'new body', 'v1')

      expect(res).toEqual({ etag: 'v2' })
      const f = get(fileStore)
      expect(f.etag).toBe('v2')
      expect(f.dirty).toBe(false)
      expect(f.savedContent).toBe('new body')
    })

    it('refuses without a path rather than saving somewhere arbitrary', async () => {
      const res = await saveFile(null, 'body', 'v1')
      expect(res).toBeNull()
      expect(fileService.saveFile).not.toHaveBeenCalled()
    })

    // The one that matters. A 409 means the file changed underneath us. If the
    // buffer were marked clean here the editor would show "Saved" over work
    // that never reached disk, and the next save would clobber the other
    // change with a stale etag.
    it('keeps the buffer dirty on a 409 conflict', async () => {
      const conflict = Object.assign(new Error('conflict'), { status: 409 })
      fileService.saveFile.mockRejectedValue(conflict)
      fileStore.update((f) => ({
        ...f,
        filePath: 'main.tex',
        content: 'my edit',
        savedContent: 'original',
        etag: 'v1',
        dirty: true,
      }))

      const res = await saveFile('main.tex', 'my edit', 'v1')

      expect(res).toBeNull()
      const f = get(fileStore)
      expect(f.dirty).toBe(true)
      expect(f.savedContent).toBe('original')
      expect(f.etag).toBe('v1')
      expect(f.content).toBe('my edit')
      expect(get(uiStore).status).toMatch(/conflict/i)
    })

    it('tells the user to reload on a conflict, not just that it failed', async () => {
      fileService.saveFile.mockRejectedValue(Object.assign(new Error('conflict'), { status: 409 }))
      await saveFile('main.tex', 'body', 'v1')

      const toasts = get(uiStore).toasts ?? []
      expect(toasts.some((t) => /reload/i.test(t.message))).toBe(true)
    })

    it('keeps the buffer dirty on an ordinary failure too', async () => {
      fileService.saveFile.mockRejectedValue(new Error('disk full'))
      fileStore.update((f) => ({ ...f, filePath: 'main.tex', dirty: true, savedContent: 'old' }))

      const res = await saveFile('main.tex', 'body', 'v1')

      expect(res).toBeNull()
      expect(get(fileStore).dirty).toBe(true)
      expect(get(fileStore).savedContent).toBe('old')
      expect(get(uiStore).status).toContain('disk full')
    })
  })
})
