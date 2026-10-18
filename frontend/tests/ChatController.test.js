import { describe, it, expect, beforeEach, vi } from 'vitest'
import { get } from 'svelte/store'

vi.mock('../src/services/aiService.js', () => ({
  agentLoopStream: vi.fn(),
  streamAIPlan: vi.fn(),
  applySelectedHunks: vi.fn(),
}))

import * as aiService from '../src/services/aiService.js'
import { createChatController } from '../src/controllers/ChatController.js'
import { aiStore } from '../src/stores/aiStore.js'

/** Stand-ins for the app state the controller is built around. */
function makeCtx(path = 'main.tex') {
  const ui = {
    status: '',
    toasts: [],
    chat: false,
    setStatus(s) { this.status = s },
    toast(message, type) { this.toasts.push({ message, type }) },
    show(pane) { this[pane] = true },
  }
  const doc = { path }
  const documents = {
    active: path ? doc : null,
    open: new Set([path]),
    get(p) { return this.open.has(p) ? doc : null },
    reload: vi.fn(async () => {}),
  }
  const compile = { start: vi.fn() }
  const project = { mainOverride: null, refresh: vi.fn(async () => {}) }
  return { ui, documents, compile, project }
}

let ctx
let chat
const handleChatMessage = (...a) => chat.handleChatMessage(...a)
const requestAIPlan = (...a) => chat.requestAIPlan(...a)
const handleApplyHunks = (...a) => chat.handleApplyHunks(...a)

describe('ChatController', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    ctx = makeCtx()
    chat = createChatController(ctx)
    aiStore.update((a) => ({
      ...a,
      status: 'idle',
      messages: [],
      currentProposal: null,
      streamingMessage: '',
    }))
    aiService.agentLoopStream.mockResolvedValue(undefined)
    aiService.streamAIPlan.mockResolvedValue(undefined)
  })

  describe('handleChatMessage', () => {
    it('refuses without an open file', async () => {
      ctx.documents.active = null
      await handleChatMessage('fix my table')
      expect(aiService.agentLoopStream).not.toHaveBeenCalled()
    })

    // The wire format is the locked SSE taxonomy (masterplan 8.2). These are
    // the event names the server actually emits; a rename on either side should
    // break here rather than silently produce an empty transcript.
    it('renders every event in the locked taxonomy', async () => {
      aiService.agentLoopStream.mockImplementation(async (path, msg, onEvent) => {
        onEvent({ event: 'thinking', data: { iteration: 1, message: 'reading the error' } })
        onEvent({ event: 'progress', data: { message: 'editing' } })
        onEvent({ event: 'applying', data: {} })
        onEvent({ event: 'compiled', data: { success: true } })
        onEvent({ event: 'done', data: { success: true } })
      })

      await handleChatMessage('fix it')

      const transcript = get(aiStore).messages.at(-1).content
      expect(transcript).toContain('Iteration 1')
      expect(transcript).toContain('editing')
      expect(transcript).toContain('Applying edits')
      expect(transcript).toContain('Compiled successfully')
      expect(transcript).toContain('Done.')
    })

    it('opens the AI pane when a proposal arrives', async () => {
      aiService.agentLoopStream.mockImplementation(async (p, m, onEvent) => {
        onEvent({ event: 'proposal', data: { edit_count: 2, edits: [] } })
      })

      await handleChatMessage('fix it')

      expect(ctx.ui.chat).toBe(true)
      expect(get(aiStore).currentProposal).toBeTruthy()
    })

    it('reports a failed compile rather than claiming success', async () => {
      aiService.agentLoopStream.mockImplementation(async (p, m, onEvent) => {
        onEvent({ event: 'compiled', data: { success: false, error_summary: 'Missing $' } })
      })

      await handleChatMessage('fix it')

      expect(get(aiStore).messages.at(-1).content).toContain('Missing $')
    })

    it('leaves the store idle after a stream error instead of stuck streaming', async () => {
      aiService.agentLoopStream.mockImplementation(async (p, m, onEvent, onError) => {
        onError(new Error('socket closed'))
      })

      await handleChatMessage('fix it')

      const a = get(aiStore)
      expect(a.status).not.toBe('streaming')
      expect(a.streamingMessage).toBe('')
      expect(ctx.ui.status).toBe('Ready')
    })

    // A rejected call is not the same as a stream error, and App.svelte invokes
    // this straight from an event handler with no catch. Left unhandled, the
    // status returned to "Ready" and the user was told nothing at all.
    it('tells the user when the request itself fails', async () => {
      aiService.agentLoopStream.mockRejectedValue(new Error('connection refused'))

      await expect(handleChatMessage('fix it')).resolves.toBeUndefined()

      const toasts = ctx.ui.toasts
      expect(toasts.some((t) => /connection refused/i.test(t.message))).toBe(true)
      expect(get(aiStore).messages.at(-1).content).toContain('connection refused')
      expect(get(aiStore).status).not.toBe('streaming')
      expect(ctx.ui.status).toBe('Ready')
    })
  })

  describe('requestAIPlan', () => {
    it('returns the proposal and shows it', async () => {
      aiService.streamAIPlan.mockImplementation(async (p, i, onEvent) => {
        onEvent({ event: 'proposal', data: { edit_count: 1 } })
      })

      const proposal = await requestAIPlan('expand macros')

      expect(proposal).toEqual({ edit_count: 1 })
      expect(ctx.ui.chat).toBe(true)
    })

    it('returns null without an open file', async () => {
      ctx.documents.active = null
      expect(await requestAIPlan('expand macros')).toBeNull()
    })

    it('tells the user when the plan request itself fails', async () => {
      aiService.streamAIPlan.mockRejectedValue(new Error('connection refused'))

      await expect(requestAIPlan('expand macros')).resolves.toBeNull()

      const toasts = ctx.ui.toasts
      expect(toasts.some((t) => /connection refused/i.test(t.message))).toBe(true)
    })

    it('surfaces a plan error to the user', async () => {
      aiService.streamAIPlan.mockImplementation(async (p, i, onEvent) => {
        onEvent({ event: 'error', data: { message: 'model unavailable' } })
      })

      await requestAIPlan('expand macros')

      expect(ctx.ui.status).toContain('model unavailable')
    })
  })

  // This is the function that writes to the user's document.
  describe('handleApplyHunks', () => {
    const proposal = {
      edits: [{ file_path: 'main.tex', diff: '@@' }],
      selectedHunkIds: ['h1'],
      expectedEtag: 'v1',
    }

    it('refuses when there is nothing to apply', async () => {
      expect(await handleApplyHunks({ edits: [], selectedHunkIds: ['h1'] })).toBe(false)
      expect(aiService.applySelectedHunks).not.toHaveBeenCalled()
    })

    it('refuses when no hunk was selected', async () => {
      expect(await handleApplyHunks({ edits: proposal.edits, selectedHunkIds: [] })).toBe(false)
      expect(aiService.applySelectedHunks).not.toHaveBeenCalled()
    })

    it('re-reads every open file it wrote, rather than trusting local state', async () => {
      aiService.applySelectedHunks.mockResolvedValue({ success: true, files: [{ path: 'main.tex' }, { path: 'ch.tex' }] })
      ctx.documents.open.add('ch.tex')

      expect(await handleApplyHunks(proposal)).toBe(true)

      // The server wrote the files; re-reading is what keeps the editor and
      // the files on disk from drifting apart.
      const reloaded = ctx.documents.reload.mock.calls.map((c) => c[0]).sort()
      expect(reloaded).toEqual(['ch.tex', 'main.tex'])
      expect(get(aiStore).currentProposal).toBeNull()
      expect(ctx.project.refresh).toHaveBeenCalled()
      expect(ctx.compile.start).toHaveBeenCalledWith('main.tex', { main: null })
    })

    it('passes the review etag so the write is conditional', async () => {
      aiService.applySelectedHunks.mockResolvedValue({ success: true })

      await handleApplyHunks(proposal)

      expect(aiService.applySelectedHunks).toHaveBeenCalledWith(
        'main.tex',
        proposal.edits,
        ['h1'],
        'v1',
        { proposalId: null },
      )
    })

    // The dangerous one. A 409 means the file moved while the proposal was
    // being reviewed, so the diff no longer describes the file. Clearing the
    // proposal or marking the buffer clean here would lose the user's review
    // and imply an edit landed that did not.
    it('keeps the proposal and the buffer untouched on a 409', async () => {
      aiStore.update((a) => ({ ...a, currentProposal: { edit_count: 1 } }))
      aiService.applySelectedHunks.mockRejectedValue(
        Object.assign(new Error('conflict'), { status: 409 }),
      )

      expect(await handleApplyHunks(proposal)).toBe(false)

      expect(get(aiStore).currentProposal).toEqual({ edit_count: 1 })
      expect(ctx.documents.reload).not.toHaveBeenCalled()
      expect(ctx.compile.start).not.toHaveBeenCalled()
      expect(ctx.ui.status).toMatch(/changed during review/i)
    })

    it('tells the user to re-plan after a conflict, not merely to retry', async () => {
      aiService.applySelectedHunks.mockRejectedValue(
        Object.assign(new Error('conflict'), { status: 409 }),
      )
      await handleApplyHunks(proposal)

      const toasts = ctx.ui.toasts
      expect(toasts.some((t) => /re-plan/i.test(t.message))).toBe(true)
    })

    it('does not compile when the server reports failure', async () => {
      aiService.applySelectedHunks.mockResolvedValue({ success: false, error: 'hunk is stale' })

      expect(await handleApplyHunks(proposal)).toBe(false)

      expect(ctx.compile.start).not.toHaveBeenCalled()
      expect(ctx.ui.status).toContain('hunk is stale')
    })
  })
})
