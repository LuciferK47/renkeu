import { describe, it, expect, beforeEach } from 'vitest'
import { get } from 'svelte/store'
import { fileStore, setFile, updateContent, markDirty, markSaved } from '../src/stores/fileStore.js'
import { compileStore, resetCompile } from '../src/stores/compileStore.js'
import { uiStore, setActiveRightTab, pushToast, setStatus } from '../src/stores/uiStore.js'

describe('fileStore', () => {
  beforeEach(() => setFile('main.tex', 'original', 'v1'))

  it('opens a file clean', () => {
    const f = get(fileStore)
    expect(f.dirty).toBe(false)
    expect(f.savedContent).toBe('original')
  })

  it('marks dirty when the content diverges from what is on disk', () => {
    updateContent('edited')
    expect(get(fileStore).dirty).toBe(true)
  })

  // The subtle one: dirty is a comparison, not a latch. Typing a character and
  // deleting it again leaves the buffer identical to disk, so the editor should
  // stop claiming unsaved changes rather than nagging until an explicit save.
  it('becomes clean again when an edit is undone back to the saved text', () => {
    updateContent('edited')
    expect(get(fileStore).dirty).toBe(true)
    updateContent('original')
    expect(get(fileStore).dirty).toBe(false)
  })

  it('markDirty forces the flag regardless of content', () => {
    markDirty()
    expect(get(fileStore).dirty).toBe(true)
  })

  it('markSaved moves the baseline so later edits compare against the new text', () => {
    updateContent('edited')
    markSaved('edited', 'v2')

    const f = get(fileStore)
    expect(f.dirty).toBe(false)
    expect(f.savedContent).toBe('edited')
    expect(f.etag).toBe('v2')

    updateContent('original')
    expect(get(fileStore).dirty).toBe(true)
  })

  it('treats whitespace-only changes as real changes', () => {
    updateContent('original ')
    expect(get(fileStore).dirty).toBe(true)
  })
})

describe('compileStore', () => {
  beforeEach(() => resetCompile())

  it('starts idle with nothing to report', () => {
    const c = get(compileStore)
    expect(c.status).toBe('idle')
    expect(c.errors).toEqual([])
    expect(c.jobId).toBeNull()
  })

  it('reset clears a previous failure so a new document starts clean', () => {
    compileStore.update((c) => ({
      ...c,
      status: 'error',
      errors: [{ message: 'Undefined control sequence' }],
      jobId: 'job-1',
      pdfUrl: '/old.pdf',
    }))

    resetCompile()

    const c = get(compileStore)
    expect(c.status).toBe('idle')
    // Carrying the last document's errors into a new one is how people chase
    // a failure they already fixed.
    expect(c.errors).toEqual([])
    expect(c.jobId).toBeNull()
    expect(c.pdfUrl).toBeNull()
  })

  it('uses the one status vocabulary the rest of the app checks', () => {
    // CompileController disables the button and coalesces on exactly this
    // string; a store helper that wrote 'pending' would silently defeat both.
    compileStore.update((c) => ({ ...c, status: 'compiling' }))
    expect(get(compileStore).status).toBe('compiling')
  })
})

describe('uiStore', () => {
  beforeEach(() => {
    uiStore.update((ui) => ({ ...ui, toasts: [], status: '' }))
  })

  it('switches the right-hand pane', () => {
    setActiveRightTab('errors')
    expect(get(uiStore).activeRightTab).toBe('errors')
  })

  it('keeps toasts in arrival order', () => {
    pushToast('first', 'info')
    pushToast('second', 'error')
    const messages = get(uiStore).toasts.map((t) => t.message)
    expect(messages).toEqual(['first', 'second'])
  })

  it('gives every toast a distinct id so removing one leaves the rest', () => {
    pushToast('a', 'info')
    pushToast('b', 'info')
    const ids = get(uiStore).toasts.map((t) => t.id)
    expect(new Set(ids).size).toBe(ids.length)
  })

  it('records a status line', () => {
    setStatus('Compiling…')
    expect(get(uiStore).status).toBe('Compiling…')
  })
})
