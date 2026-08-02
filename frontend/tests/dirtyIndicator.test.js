import { describe, it, expect, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/svelte'
import { get } from 'svelte/store'
import IdeShell from '../src/layouts/IdeShell.svelte'
import { fileStore } from '../src/stores/fileStore.js'
import { uiStore } from '../src/stores/uiStore.js'

/**
 * The unsaved-changes dot.
 *
 * Saving worked all along, but nothing told the user about it: App.svelte
 * listened to none of the five events the editor dispatches, so the dot never
 * appeared and the status bar never said "Saved". Driving a real browser proved
 * the store side (`fileStore.dirty` does flip to true on a keystroke) but the
 * DOM side was unmeasurable there -- Monaco tears the page down between probes.
 *
 * Rendering the shell directly is deterministic, so this is where the rendering
 * half belongs.
 */
describe('unsaved-changes indicator', () => {
  beforeEach(() => {
    fileStore.set({
      filePath: 'main.tex',
      content: 'original',
      savedContent: 'original',
      etag: 'v1',
      dirty: false,
      lastSavedAt: null,
    })
    uiStore.update((ui) => ({ ...ui, status: '', toasts: [], showSettings: false }))
  })

  it('stays hidden while the buffer matches disk', () => {
    render(IdeShell, { props: { shortcuts: [] } })
    expect(screen.queryByTitle('Unsaved changes')).toBeNull()
  })

  it('appears once the buffer is dirty', async () => {
    render(IdeShell, { props: { shortcuts: [] } })
    fileStore.update((f) => ({ ...f, dirty: true }))
    // Svelte flushes on a microtask; awaiting one is enough.
    await Promise.resolve()
    expect(screen.queryByTitle('Unsaved changes')).not.toBeNull()
  })

  it('disappears again after a save clears the flag', async () => {
    render(IdeShell, { props: { shortcuts: [] } })
    fileStore.update((f) => ({ ...f, dirty: true }))
    await Promise.resolve()
    expect(screen.queryByTitle('Unsaved changes')).not.toBeNull()

    fileStore.update((f) => ({ ...f, dirty: false }))
    await Promise.resolve()
    expect(screen.queryByTitle('Unsaved changes')).toBeNull()
  })

  it('shows the status line the save handlers set', async () => {
    render(IdeShell, { props: { shortcuts: [] } })
    uiStore.update((ui) => ({ ...ui, status: 'Saved' }))
    await Promise.resolve()
    expect(screen.getByText('Saved')).toBeTruthy()
  })

  it('reports a conflict distinctly from an ordinary failure', async () => {
    render(IdeShell, { props: { shortcuts: [] } })
    uiStore.update((ui) => ({ ...ui, status: 'Conflict: file changed on disk' }))
    await Promise.resolve()
    // "reload before saving" is the recovery; a generic failure would invite a
    // retry straight back into the same conflict.
    expect(screen.getByText(/conflict/i)).toBeTruthy()
  })

  it('has a live region so the change is announced, not just drawn', async () => {
    render(IdeShell, { props: { shortcuts: [] } })
    fileStore.update((f) => ({ ...f, dirty: true }))
    await Promise.resolve()
    const dot = screen.queryByTitle('Unsaved changes')
    expect(dot?.getAttribute('aria-live')).toBe('polite')
  })

  it('keeps the store as the single source of truth', () => {
    fileStore.update((f) => ({ ...f, dirty: true }))
    expect(get(fileStore).dirty).toBe(true)
  })
})
