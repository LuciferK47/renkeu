import { describe, it, expect, vi, beforeEach } from 'vitest'
import { consumeSSE } from '../src/services/_stream.js'
import { streamCompilationStatus } from '../src/services/compileService.js'
import { agentLoopStream } from '../src/services/aiService.js'

/** A Response whose body yields `chunks` then ends, like a real fetch stream. */
function sseResponse(chunks, ok = true) {
  return {
    ok,
    status: ok ? 200 : 500,
    statusText: ok ? 'OK' : 'Server Error',
    body: {
      getReader() {
        let i = 0
        return {
          read: async () =>
            i < chunks.length
              ? { done: false, value: new TextEncoder().encode(chunks[i++]) }
              : { done: true },
          releaseLock() {},
        }
      },
    },
  }
}

const frame = (event, data) => `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`

describe('consumeSSE', () => {
  it('reports a stream that reached a terminal event', async () => {
    const events = []
    const finished = await consumeSSE(
      sseResponse([frame('progress', { p: 0.5 }), frame('done', { success: true })]),
      (m) => events.push(m.event),
    )
    expect(finished).toBe(true)
    expect(events).toEqual(['progress', 'done'])
  })

  // The failure that used to be invisible: the reader simply reports done, so
  // `for await` ends without throwing and the caller cannot tell a finished run
  // from a severed one.
  it('reports a stream that stopped early', async () => {
    const events = []
    const finished = await consumeSSE(
      sseResponse([frame('thinking', { iteration: 1 })]),
      (m) => events.push(m.event),
    )
    expect(finished).toBe(false)
    expect(events).toEqual(['thinking'])
  })

  it('treats an error frame as a proper ending', async () => {
    const finished = await consumeSSE(sseResponse([frame('error', { message: 'boom' })]), () => {})
    expect(finished).toBe(true)
  })

  it('treats awaiting_approval as an ending, since the run pauses there', async () => {
    const finished = await consumeSSE(
      sseResponse([frame('proposal', { edit_count: 1 }), frame('awaiting_approval', {})]),
      () => {},
    )
    expect(finished).toBe(true)
  })
})

describe('agent stream truncation', () => {
  beforeEach(() => vi.restoreAllMocks())

  // Re-POSTing /agent/stream starts a second run against the same file, and
  // with auto_apply that can apply the same edits twice. Reporting is the only
  // safe response.
  it('reports a severed run instead of retrying it', async () => {
    const fetchMock = vi.fn().mockResolvedValue(sseResponse([frame('thinking', { iteration: 1 })]))
    vi.stubGlobal('fetch', fetchMock)

    const onError = vi.fn()
    await agentLoopStream('main.tex', 'fix it', () => {}, onError)

    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(onError).toHaveBeenCalledTimes(1)
    expect(onError.mock.calls[0][0].message).toMatch(/connection lost/i)
    // It may still be running, so the wording must not claim it failed.
    expect(onError.mock.calls[0][0].message).toMatch(/may still be running/i)
  })

  it('stays quiet when the run finishes properly', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(
      sseResponse([frame('proposal', { edit_count: 1 }), frame('done', { success: true })]),
    ))
    const onError = vi.fn()
    await agentLoopStream('main.tex', 'fix it', () => {}, onError)
    expect(onError).not.toHaveBeenCalled()
  })
})

describe('compile stream re-attach', () => {
  beforeEach(() => vi.restoreAllMocks())

  it('re-attaches to the same job when the stream drops', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(sseResponse([frame('progress', { progress: 0.3 })]))
      .mockResolvedValueOnce(sseResponse([frame('compiled', { success: true }), frame('done', {})]))
    vi.stubGlobal('fetch', fetchMock)

    const events = []
    await streamCompilationStatus('job-1', (m) => events.push(m.event), undefined, { sleep: async () => {} })

    // Safe to retry precisely because this endpoint only observes a job that
    // already exists; it cannot start a second compile.
    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect(events).toEqual(['progress', 'compiled', 'done'])
  })

  it('gives up after a bounded number of attempts rather than looping forever', async () => {
    const fetchMock = vi.fn().mockResolvedValue(sseResponse([frame('progress', { progress: 0.1 })]))
    vi.stubGlobal('fetch', fetchMock)

    const onError = vi.fn()
    await streamCompilationStatus('job-1', () => {}, onError, { sleep: async () => {}, maxAttempts: 3 })

    expect(fetchMock).toHaveBeenCalledTimes(3)
    expect(onError).toHaveBeenCalledTimes(1)
    expect(onError.mock.calls[0][0].message).toMatch(/lost contact/i)
  })

  it('does not re-attach when the job completes first time', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      sseResponse([frame('compiled', { success: true }), frame('done', {})]),
    )
    vi.stubGlobal('fetch', fetchMock)

    await streamCompilationStatus('job-1', () => {}, undefined, { sleep: async () => {} })
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })

  it('retries a failed connection too, not just a truncated one', async () => {
    const fetchMock = vi
      .fn()
      .mockRejectedValueOnce(new Error('connection refused'))
      .mockResolvedValueOnce(sseResponse([frame('done', { success: true })]))
    vi.stubGlobal('fetch', fetchMock)

    const onError = vi.fn()
    await streamCompilationStatus('job-1', () => {}, onError, { sleep: async () => {} })

    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect(onError).not.toHaveBeenCalled()
  })

  it('backs off between attempts instead of hammering the server', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse([frame('progress', {})])))
    const delays = []
    await streamCompilationStatus('job-1', () => {}, vi.fn(), {
      sleep: async (ms) => delays.push(ms),
      maxAttempts: 3,
    })
    expect(delays).toEqual([250, 500])
  })
})
