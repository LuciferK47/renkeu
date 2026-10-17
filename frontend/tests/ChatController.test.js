import { describe, it, expect, beforeEach, vi } from 'vitest'
import { get } from 'svelte/store'

vi.mock('../src/services/aiService.js', () => ({
  agentLoopStream: vi.fn(),
  streamAIPlan: vi.fn(),
  applySelectedHunks: vi.fn(),
}))
vi.mock('../src/services/fileService.js', () => ({
  loadFile: vi.fn(),
  saveFile: vi.fn(),
  listFiles: vi.fn(),
  searchWorkspace: vi.fn(),
}))
vi.mock('../src/controllers/CompileController.js', () => ({
  compileDocument: vi.fn(),
}))

import * as aiService from '../src/services/aiService.js'
import * as fileService from '../src/services/fileService.js'
import { compileDocument } from '../src/controllers/CompileController.js'
import {
  handleChatMessage,
  requestAIPlan,
  handleApplyHunks,
} from '../src/controllers/ChatController.js'
import { aiStore } from '../src/stores/aiStore.js'
import { fileStore } from '../src/stores/fileStore.js'
import { uiStore } from '../src/stores/uiStore.js'

const openFile = (path = 'main.tex') =>
  fileStore.set({
    filePath: path,
    content: 'original',
    savedContent: 'original',
    etag: 'v1',
    dirty: false,
    lastSavedAt: null,
  })

describe('ChatController', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    openFile()
    uiStore.update((ui) => ({ ...ui, status: '', toasts: [], activeRightTab: 'diff' }))
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
      fileStore.update((f) => ({ ...f, filePath: null }))
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

      expect(get(uiStore).activeRightTab).toBe('ai')
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
      expect(get(uiStore).status).toBe('Ready')
    })

    // A rejected call is not the same as a stream error, and App.svelte invokes
    // this straight from an event handler with no catch. Left unhandled, the
    // status returned to "Ready" and the user was told nothing at all.
    it('tells the user when the request itself fails', async () => {
      aiService.agentLoopStream.mockRejectedValue(new Error('connection refused'))

      await expect(handleChatMessage('fix it')).resolves.toBeUndefined()

      const toasts = get(uiStore).toasts ?? []
      expect(toasts.some((t) => /connection refused/i.test(t.message))).toBe(true)
      expect(get(aiStore).messages.at(-1).content).toContain('connection refused')
      expect(get(aiStore).status).not.toBe('streaming')
      expect(get(uiStore).status).toBe('Ready')
    })
  })

  describe('requestAIPlan', () => {
    it('returns the proposal and shows it', async () => {
      aiService.streamAIPlan.mockImplementation(async (p, i, onEvent) => {
        onEvent({ event: 'proposal', data: { edit_count: 1 } })
      })

      const proposal = await requestAIPlan('expand macros')

      expect(proposal).toEqual({ edit_count: 1 })
      expect(get(uiStore).activeRightTab).toBe('ai')
    })

    it('returns null without an open file', async () => {
      fileStore.update((f) => ({ ...f, filePath: null }))
      expect(await requestAIPlan('expand macros')).toBeNull()
    })

    it('tells the user when the plan request itself fails', async () => {
      aiService.streamAIPlan.mockRejectedValue(new Error('connection refused'))

      await expect(requestAIPlan('expand macros')).resolves.toBeNull()

      const toasts = get(uiStore).toasts ?? []
      expect(toasts.some((t) => /connection refused/i.test(t.message))).toBe(true)
    })

    it('surfaces a plan error to the user', async () => {
      aiService.streamAIPlan.mockImplementation(async (p, i, onEvent) => {
        onEvent({ event: 'error', data: { message: 'model unavailable' } })
      })

      await requestAIPlan('expand macros')

      expect(get(uiStore).status).toContain('model unavailable')
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

    it('reloads from disk after applying rather than trusting local state', async () => {
      aiService.applySelectedHunks.mockResolvedValue({ success: true })
      fileService.loadFile.mockResolvedValue({ content: 'patched', eTag: 'v2' })

      expect(await handleApplyHunks(proposal)).toBe(true)

      const f = get(fileStore)
      // The server applied the edit; re-reading is what keeps the editor and
      // the file on disk from drifting apart.
      expect(f.content).toBe('patched')
      expect(f.savedContent).toBe('patched')
      expect(f.etag).toBe('v2')
      expect(f.dirty).toBe(false)
      expect(get(aiStore).currentProposal).toBeNull()
      expect(compileDocument).toHaveBeenCalledWith('main.tex')
    })

    it('passes the review etag so the write is conditional', async () => {
      aiService.applySelectedHunks.mockResolvedValue({ success: true })
      fileService.loadFile.mockResolvedValue({ content: 'patched', eTag: 'v2' })

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
      expect(get(fileStore).content).toBe('original')
      expect(fileService.loadFile).not.toHaveBeenCalled()
      expect(compileDocument).not.toHaveBeenCalled()
      expect(get(uiStore).status).toMatch(/changed during review/i)
    })

    it('tells the user to re-plan after a conflict, not merely to retry', async () => {
      aiService.applySelectedHunks.mockRejectedValue(
        Object.assign(new Error('conflict'), { status: 409 }),
      )
      await handleApplyHunks(proposal)

      const toasts = get(uiStore).toasts ?? []
      expect(toasts.some((t) => /re-plan/i.test(t.message))).toBe(true)
    })

    it('does not compile when the server reports failure', async () => {
      aiService.applySelectedHunks.mockResolvedValue({ success: false, error: 'hunk is stale' })

      expect(await handleApplyHunks(proposal)).toBe(false)

      expect(compileDocument).not.toHaveBeenCalled()
      expect(get(uiStore).status).toContain('hunk is stale')
    })
  })
})
