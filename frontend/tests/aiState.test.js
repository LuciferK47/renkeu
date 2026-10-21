import { describe, it, expect, beforeEach, vi } from 'vitest'
import { APIError } from '../src/lib/api/client.js'
import { AiState } from '../src/lib/state/ai.svelte.js'

/** Stand-ins for the app state the AI is built around. */
function setup(path = 'main.tex') {
  const ui = {
    status: '',
    toasts: [],
    chat: false,
    setStatus(s) { this.status = s },
    toast(message, type) { this.toasts.push({ message, type }) },
    show(pane) { this[pane] = true },
  }
  const doc = { path }
  const open = new Set([path])
  const documents = { active: path ? doc : null, get: (p) => (open.has(p) ? doc : null), reload: vi.fn(async () => {}), open }
  const compile = { start: vi.fn() }
  const project = { mainOverride: null, refresh: vi.fn(async () => {}) }
  const service = {
    agentLoopStream: vi.fn(async () => {}),
    streamAIPlan: vi.fn(async () => {}),
    applySelectedHunks: vi.fn(),
  }
  const ai = new AiState({ documents, compile, project, ui, service })
  return { ai, ui, documents, compile, project, service }
}

const emit = (events) => async (path, instruction, onEvent) => events.forEach(([event, data]) => onEvent({ event, data }))
const lastRun = (ai) => ai.messages.at(-1).run
const texts = (ai) => lastRun(ai).steps.map((s) => s.text)

describe('AiState.send: the timeline', () => {
  let t
  beforeEach(() => (t = setup()))

  it('refuses without an open file and records nothing', async () => {
    t.documents.active = null
    await t.ai.send('fix it')
    expect(t.service.agentLoopStream).not.toHaveBeenCalled()
    expect(t.ai.messages).toEqual([])
    expect(t.ui.toasts[0].message).toMatch(/open a file/i)
  })

  it('turns the locked event taxonomy into ordered steps, closing each as the next begins', async () => {
    t.service.agentLoopStream.mockImplementation(emit([
      ['thinking', { iteration: 1, message: 'reading the error' }],
      ['step', { iteration: 1, edit_count: 2 }],
      ['applying', { message: 'on a copy' }],
      ['compiled', { success: false, error_summary: 'Missing $' }],
      ['thinking', { iteration: 2, message: 'trying again' }],
      ['applying', {}],
      ['compiled', { success: true }],
      ['proposal', { edit_count: 1, edits: [{}], files: [{ path: 'main.tex' }] }],
      ['awaiting_approval', {}],
      ['done', { success: true, awaiting_approval: true }],
    ]))

    await t.ai.send('fix it')

    const steps = lastRun(t.ai).steps
    expect(steps.map((s) => [s.text, s.state])).toEqual([
      ['Attempt 1: 2 edits proposed', 'ok'],
      ['It did not compile', 'failed'],
      ['Attempt 2', 'ok'],
      ['It compiles', 'ok'],
      ['Proposed 1 edit', 'ok'],
    ])
    expect(steps[1].detail).toBe('Missing $')
    expect(lastRun(t.ai).outcome).toBe('review')
    expect(steps.every((s) => s.state !== 'active')).toBe(true)
  })

  it('shows the user message first, then the assistant run', async () => {
    await t.ai.send('add a title')
    expect(t.ai.messages.map((m) => [m.role, m.text ?? null])).toEqual([['user', 'add a title'], ['assistant', null]])
  })

  it('opens the assistant and holds the proposal for review', async () => {
    t.service.agentLoopStream.mockImplementation(emit([
      ['proposal', { edit_count: 2, edits: [{}, {}], files: [{ path: 'a.tex' }, { path: 'b.tex' }], proposal_id: 'p1' }],
    ]))
    await t.ai.send('fix it')
    expect(t.ui.chat).toBe(true)
    expect(t.ai.proposal).toMatchObject({ proposal_id: 'p1', file: 'main.tex' })
    expect(t.ai.status).toBe('review')
    expect(texts(t.ai)).toEqual(['Proposed 2 edits in 2 files'])
  })

  it('reports a stream error as a failed step and ends idle, not stuck running', async () => {
    t.service.agentLoopStream.mockImplementation(async (p, i, onEvent, onError) => onError(new Error('socket closed')))
    await t.ai.send('fix it')
    expect(lastRun(t.ai).outcome).toBe('failed')
    expect(lastRun(t.ai).steps.at(-1)).toMatchObject({ text: 'socket closed', state: 'failed' })
    expect(t.ai.status).toBe('idle')
    expect(t.ui.status).toBe('Ready')
  })

  it('tells the user when the request itself fails', async () => {
    t.service.agentLoopStream.mockRejectedValue(new Error('connection refused'))
    await expect(t.ai.send('fix it')).resolves.toBeUndefined()
    expect(t.ui.toasts.some((x) => /connection refused/i.test(x.message))).toBe(true)
    expect(lastRun(t.ai).note).toBe('connection refused')
    expect(t.ai.status).toBe('idle')
  })

  it('a run that ends unsuccessfully says why', async () => {
    t.service.agentLoopStream.mockImplementation(emit([['done', { success: false, message: 'Max iterations reached' }]]))
    await t.ai.send('fix it')
    expect(lastRun(t.ai)).toMatchObject({ outcome: 'failed', note: 'Max iterations reached' })
  })

  it('ignores a second request while one is running', async () => {
    let release
    t.service.agentLoopStream.mockImplementation(() => new Promise((r) => (release = r)))
    const first = t.ai.send('one')
    await Promise.resolve()
    await t.ai.send('two')
    expect(t.service.agentLoopStream).toHaveBeenCalledTimes(1)
    release()
    await first
  })
})

describe('AiState.fit: fitting a page limit', () => {
  it('asks the server for the limit, with room for more attempts, and says so in the chat', async () => {
    const t = setup()
    await t.ai.fit(8, 'keep the abstract')
    expect(t.service.agentLoopStream).toHaveBeenCalledWith(
      'main.tex', 'keep the abstract', expect.any(Function), expect.any(Function),
      expect.objectContaining({ pages: 8, maxIterations: 6 })
    )
    expect(t.ai.messages[0].text).toBe('Fit to 8 pages: keep the abstract')
  })

  it('words a missed page count and a refused edit differently from a compile error', async () => {
    const t = setup()
    t.service.agentLoopStream.mockImplementation(emit([
      ['thinking', { iteration: 1 }],
      ['compiled', { success: false, pages: 9, layout: true, error_summary: 'The document compiles to 9 pages' }],
      ['thinking', { iteration: 2 }],
      ['compiled', { success: false, pages: null, layout: true, error_summary: 'The edits removed content' }],
      ['thinking', { iteration: 3 }],
      ['compiled', { success: true }],
      ['proposal', { edit_count: 2, edits: [{}], fit: { limit: 8, pages_before: 9, pages_after: 8 } }],
      ['done', { success: true, awaiting_approval: true }],
    ]))
    await t.ai.fit(8)
    expect(texts(t.ai).filter((x) => !x.startsWith('Attempt'))).toEqual([
      'Still 9 pages', 'Edit not accepted', 'It compiles', 'Proposed 2 edits · 9 → 8 pages',
    ])
    expect(t.ai.proposal.fit.pages_after).toBe(8)
  })

  it('a document that already fits ends as a success with the reason', async () => {
    const t = setup()
    t.service.agentLoopStream.mockImplementation(emit([
      ['done', { success: true, message: 'Already fits: 5 page(s), limit 8.', fit: { limit: 8, pages_before: 5, pages_after: 5 } }],
    ]))
    await t.ai.fit(8)
    expect(lastRun(t.ai).outcome).toBe('success')
    expect(lastRun(t.ai).note).toMatch(/already fits/i)
  })
})

describe('AiState.cancel', () => {
  it('aborts the request, marks the run stopped and leaves nothing running', async () => {
    const { ai, service } = setup()
    let seen
    service.agentLoopStream.mockImplementation((path, instruction, onEvent, onError, options) => {
      seen = options.signal
      onEvent({ event: 'thinking', data: { iteration: 1, message: 'working' } })
      return new Promise((resolve) => options.signal.addEventListener('abort', () => resolve()))
    })

    const run = ai.send('fix it')
    await Promise.resolve()
    expect(ai.running).toBe(true)
    ai.cancel()
    await run

    expect(seen.aborted).toBe(true)
    expect(lastRun(ai).outcome).toBe('cancelled')
    expect(lastRun(ai).steps.at(-1)).toMatchObject({ text: 'Stopped', state: 'failed' })
    expect(ai.status).toBe('idle')
    expect(ai.proposal).toBeNull()
  })

  it('is harmless when nothing is running', () => {
    expect(() => setup().ai.cancel()).not.toThrow()
  })

  it('a new conversation stops the run and clears the pane', async () => {
    const { ai, service } = setup()
    service.agentLoopStream.mockImplementation((p, i, e, er, o) => new Promise((r) => o.signal.addEventListener('abort', () => r())))
    const run = ai.send('x')
    await Promise.resolve()
    ai.newConversation()
    await run
    expect(ai.messages).toEqual([])
    expect(ai.status).toBe('idle')
  })
})

describe('AiState.plan', () => {
  it('returns the proposal and shows it', async () => {
    const { ai, service, ui } = setup()
    service.streamAIPlan.mockImplementation(emit([['proposal', { edit_count: 1, edits: [{}] }], ['done', { success: true }]]))
    const proposal = await ai.plan('expand macros')
    expect(proposal).toMatchObject({ edit_count: 1 })
    expect(ui.chat).toBe(true)
    expect(lastRun(ai).outcome).toBe('review')
  })

  it('returns null without an open file, and when there is no proposal', async () => {
    const t = setup()
    expect(await t.ai.plan('x')).toBeNull()
    t.documents.active = { path: 'main.tex' }
    expect(await t.ai.plan('x')).toBeNull()
  })
})

// This is the code path that writes to the user's documents.
describe('AiState.apply', () => {
  const args = { edits: [{ file_path: 'main.tex' }], selectedHunkIds: ['h1'], expectedEtag: 'preview-etag' }
  let t
  beforeEach(() => {
    t = setup()
    t.ai.showProposal({ proposal_id: 'p1', etag: 'base', file: 'main.tex', edits: args.edits })
  })

  it('refuses when there is nothing to apply or nothing selected', async () => {
    expect(await t.ai.apply({ edits: [], selectedHunkIds: ['h1'] })).toBe(false)
    expect(await t.ai.apply({ edits: args.edits, selectedHunkIds: [] })).toBe(false)
    expect(t.service.applySelectedHunks).not.toHaveBeenCalled()
  })

  it("checks against the proposal's own base, not whatever the file is now", async () => {
    t.service.applySelectedHunks.mockResolvedValue({ success: true })
    await t.ai.apply(args)
    expect(t.service.applySelectedHunks).toHaveBeenCalledWith('main.tex', args.edits, ['h1'], 'base', { proposalId: 'p1' })
  })

  it('re-reads every open file it wrote and recompiles', async () => {
    t.service.applySelectedHunks.mockResolvedValue({ success: true, files: [{ path: 'main.tex' }, { path: 'ch.tex' }] })
    t.documents.open.add('ch.tex')

    expect(await t.ai.apply(args)).toBe(true)

    expect(t.documents.reload.mock.calls.map((c) => c[0]).sort()).toEqual(['ch.tex', 'main.tex'])
    expect(t.project.refresh).toHaveBeenCalled()
    expect(t.compile.start).toHaveBeenCalledWith('main.tex', { main: null })
    expect(t.ai.proposal).toBeNull()
    expect(t.ai.status).toBe('idle')
  })

  it('retries once with rebase when the file moved and the server still has the proposal', async () => {
    t.service.applySelectedHunks
      .mockRejectedValueOnce(new APIError('conflict', 409, null))
      .mockResolvedValueOnce({ success: true })

    expect(await t.ai.apply(args)).toBe(true)

    expect(t.service.applySelectedHunks).toHaveBeenCalledTimes(2)
    expect(t.service.applySelectedHunks.mock.calls[1][4]).toEqual({ proposalId: 'p1', rebase: true })
    expect(t.ui.toasts.some((x) => /moved onto it/.test(x.message))).toBe(true)
  })

  it('keeps the proposal and the buffers untouched when the rebase is refused too', async () => {
    t.service.applySelectedHunks.mockRejectedValue(new APIError('conflict', 409, null))
    expect(await t.ai.apply(args)).toBe(false)
    expect(t.service.applySelectedHunks).toHaveBeenCalledTimes(2)
    expect(t.ai.proposal).not.toBeNull()
    expect(t.ai.status).toBe('review')
    expect(t.documents.reload).not.toHaveBeenCalled()
    expect(t.compile.start).not.toHaveBeenCalled()
    expect(t.ui.toasts.some((x) => /re-plan/i.test(x.message))).toBe(true)
  })

  it('does not retry a proposal the server does not hold', async () => {
    t.ai.showProposal({ summary: 'visual fix', etag: 'e', file: 'main.tex', edits: args.edits })
    t.service.applySelectedHunks.mockRejectedValue(new APIError('conflict', 409, null))
    expect(await t.ai.apply(args)).toBe(false)
    expect(t.service.applySelectedHunks).toHaveBeenCalledTimes(1)
  })

  it('does not compile when the server reports failure', async () => {
    t.service.applySelectedHunks.mockResolvedValue({ success: false, error: 'hunk is stale' })
    expect(await t.ai.apply(args)).toBe(false)
    expect(t.compile.start).not.toHaveBeenCalled()
    expect(t.ui.status).toContain('hunk is stale')
  })

  it('discard drops the proposal without writing anything', () => {
    t.ai.discard()
    expect(t.ai.proposal).toBeNull()
    expect(t.service.applySelectedHunks).not.toHaveBeenCalled()
  })
})
