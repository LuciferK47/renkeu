import { describe, it, expect, beforeEach, vi } from 'vitest'
import { APIClient, APIError } from '../src/lib/api/client.js'

vi.mock('../src/services/aiService.js', () => ({
  applySelectedHunks: vi.fn(),
  agentLoopStream: vi.fn(),
  streamAIPlan: vi.fn()
}))

import * as aiService from '../src/services/aiService.js'
import { createChatController } from '../src/controllers/ChatController.js'
import { aiStore } from '../src/stores/aiStore.js'

describe('apply-selected request', () => {
  beforeEach(() => vi.stubGlobal('fetch', vi.fn(async () => new Response('{"success":true}', { status: 200 }))))

  it('carries the proposal id and the rebase request only when given', async () => {
    const c = new APIClient()
    await c.applySelectedHunks('a.tex', [], ['h'], 'etag', null, { proposalId: 'p1', rebase: true })
    expect(JSON.parse(fetch.mock.calls[0][1].body)).toMatchObject({ proposal_id: 'p1', rebase: true })

    await c.applySelectedHunks('a.tex', [], ['h'], 'etag')
    const plain = JSON.parse(fetch.mock.calls[1][1].body)
    expect(plain).not.toHaveProperty('proposal_id')
    expect(plain).not.toHaveProperty('rebase')
  })
})

function makeChat() {
  const ui = { status: '', toasts: [], setStatus(s) { this.status = s }, toast(m, t) { this.toasts.push({ m, t }) }, show() {} }
  const doc = { path: 'main.tex' }
  const documents = { active: doc, get: () => doc, reload: vi.fn(async () => {}) }
  const compile = { start: vi.fn() }
  const project = { mainOverride: null, refresh: vi.fn(async () => {}) }
  return { chat: createChatController({ ui, documents, compile, project }), ui }
}

describe('applying a reviewed proposal', () => {
  beforeEach(() => vi.clearAllMocks())

  const args = { edits: [{ file_path: 'main.tex' }], selectedHunkIds: ['h1'], expectedEtag: 'preview-etag' }

  it('retries once with rebase when the file moved and the server still has the proposal', async () => {
    aiStore.update((a) => ({ ...a, currentProposal: { proposal_id: 'p1', etag: 'base' } }))
    aiService.applySelectedHunks
      .mockRejectedValueOnce(new APIError('conflict', 409, null))
      .mockResolvedValueOnce({ success: true })
    const { chat, ui } = makeChat()

    expect(await chat.handleApplyHunks(args)).toBe(true)

    expect(aiService.applySelectedHunks).toHaveBeenCalledTimes(2)
    expect(aiService.applySelectedHunks.mock.calls[0][4]).toEqual({ proposalId: 'p1' })
    expect(aiService.applySelectedHunks.mock.calls[1][4]).toEqual({ proposalId: 'p1', rebase: true })
    expect(ui.toasts.some((t) => /moved onto it/.test(t.m))).toBe(true)
  })

  it('checks against the proposal\'s own base, not whatever the file is now', async () => {
    // The preview's etag is "the file as it is at preview time"; using it would
    // make a stale proposal look fresh. The proposal's etag is what it was
    // written against.
    aiStore.update((a) => ({ ...a, currentProposal: { proposal_id: 'p1', etag: 'base' } }))
    aiService.applySelectedHunks.mockResolvedValue({ success: true })
    const { chat } = makeChat()

    await chat.handleApplyHunks(args)

    expect(aiService.applySelectedHunks.mock.calls[0][3]).toBe('base')
  })

  it('does not retry without a proposal to rebase', async () => {
    aiStore.update((a) => ({ ...a, currentProposal: { summary: 'old style, no id' } }))
    aiService.applySelectedHunks.mockRejectedValue(new APIError('conflict', 409, null))
    const { chat } = makeChat()

    expect(await chat.handleApplyHunks(args)).toBe(false)
    expect(aiService.applySelectedHunks).toHaveBeenCalledTimes(1)
  })

  it('gives up when the rebase is refused too', async () => {
    aiStore.update((a) => ({ ...a, currentProposal: { proposal_id: 'p1' } }))
    aiService.applySelectedHunks.mockRejectedValue(new APIError('conflict', 409, null))
    const { chat } = makeChat()

    expect(await chat.handleApplyHunks(args)).toBe(false)
    expect(aiService.applySelectedHunks).toHaveBeenCalledTimes(2)
  })
})
