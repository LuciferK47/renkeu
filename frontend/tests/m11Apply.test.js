import { describe, it, expect, beforeEach, vi } from 'vitest'
import { APIClient } from '../src/lib/api/client.js'


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
