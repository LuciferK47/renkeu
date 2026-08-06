import { describe, it, expect, beforeEach, vi } from 'vitest'
import { get } from 'svelte/store'

vi.mock('../src/services/compileService.js', () => ({
  startCompilation: vi.fn(),
  streamCompilationStatus: vi.fn(),
  pdfPageUrl: vi.fn((f, n) => `/api/pdf/page?file=${f}&n=${n}`),
}))

import * as compileService from '../src/services/compileService.js'
import { compileDocument } from '../src/controllers/CompileController.js'
import { compileStore } from '../src/stores/compileStore.js'
import { uiStore } from '../src/stores/uiStore.js'

/** A promise whose settlement this test controls, so "in flight" is a real state. */
function deferred() {
  let resolve
  const promise = new Promise((r) => (resolve = r))
  return { promise, resolve }
}

function resetStores() {
  compileStore.update((c) => ({
    ...c,
    status: 'idle',
    jobId: null,
    errors: [],
    warnings: [],
    progress: 0,
  }))
  uiStore.update((ui) => ({ ...ui, status: '', toasts: [], activeRightTab: 'diff' }))
}

describe('CompileController', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    resetStores()
    compileService.startCompilation.mockResolvedValue({ job_id: 'job-1' })
    compileService.streamCompilationStatus.mockResolvedValue(undefined)
  })

  it('refuses without a file rather than compiling nothing', async () => {
    await compileDocument('')
    expect(compileService.startCompilation).not.toHaveBeenCalled()
    expect(get(uiStore).status).toMatch(/select a file/i)
  })

  it('shows the PDF pane while the compile runs', async () => {
    await compileDocument('main.tex')
    expect(get(uiStore).activeRightTab).toBe('pdf')
  })

  it('records success with the page count and a PDF url', async () => {
    compileService.streamCompilationStatus.mockImplementation(async (id, onEvent) => {
      onEvent({ event: 'compiled', data: { success: true, page_count: 4 } })
    })

    await compileDocument('main.tex')

    const c = get(compileStore)
    expect(c.status).toBe('success')
    expect(c.pageCount).toBe(4)
    expect(c.progress).toBe(100)
    expect(c.pdfUrl).toContain('main.tex')
    expect(c.errors).toEqual([])
  })

  it('switches to the errors pane when the compile fails', async () => {
    compileService.streamCompilationStatus.mockImplementation(async (id, onEvent) => {
      onEvent({ event: 'compiled', data: { success: false, error_summary: 'Undefined control sequence' } })
    })

    await compileDocument('main.tex')

    const c = get(compileStore)
    expect(c.status).toBe('error')
    expect(c.errors[0].message).toBe('Undefined control sequence')
    // A failure the user cannot see is a failure they will not fix.
    expect(get(uiStore).activeRightTab).toBe('errors')
  })

  it('reports progress as a percentage', async () => {
    compileService.streamCompilationStatus.mockImplementation(async (id, onEvent) => {
      onEvent({ event: 'progress', data: { progress: 0.42, message: 'Typesetting' } })
    })

    await compileDocument('main.tex')

    expect(get(compileStore).progress).toBe(42)
    expect(get(uiStore).status).toBe('Typesetting')
  })

  it('survives a compiled event with no page count', async () => {
    compileService.streamCompilationStatus.mockImplementation(async (id, onEvent) => {
      onEvent({ event: 'compiled', data: { success: true } })
    })
    await compileDocument('main.tex')
    expect(get(compileStore).pageCount).toBe(1)
  })

  describe('coalescing', () => {
    it('does not start a second job while one is in flight', async () => {
      const gate = deferred()
      compileService.streamCompilationStatus.mockImplementation(() => gate.promise)

      const first = compileDocument('main.tex')
      await Promise.resolve()

      // Two more requests arrive mid-compile; both collapse into one queued run.
      await compileDocument('main.tex')
      await compileDocument('main.tex')
      expect(compileService.startCompilation).toHaveBeenCalledTimes(1)

      gate.resolve()
      await first
    })

    it('runs the queued compile once the first finishes', async () => {
      const gate = deferred()
      compileService.streamCompilationStatus
        .mockImplementationOnce(async (id, onEvent) => {
          await gate.promise
          // Land on a terminal status, which is what releases the queue.
          onEvent({ event: 'compiled', data: { success: true, page_count: 1 } })
        })
        .mockImplementation(async (id, onEvent) => {
          onEvent({ event: 'compiled', data: { success: true, page_count: 2 } })
        })

      const first = compileDocument('main.tex')
      await Promise.resolve()
      await compileDocument('main.tex')

      gate.resolve()
      await first

      // Once for the original, once for the coalesced follow-up -- not three
      // times for three requests.
      expect(compileService.startCompilation).toHaveBeenCalledTimes(2)
      expect(get(compileStore).pageCount).toBe(2)
    })
  })

  it('surfaces a transport failure instead of hanging in "compiling"', async () => {
    compileService.startCompilation.mockRejectedValue(new Error('connection refused'))

    await compileDocument('main.tex')

    const c = get(compileStore)
    expect(c.status).toBe('error')
    expect(c.errors[0].message).toContain('connection refused')
  })

  it('reports a stream error through the error callback', async () => {
    compileService.streamCompilationStatus.mockImplementation(async (id, onEvent, onError) => {
      onError(new Error('stream died'))
    })

    await compileDocument('main.tex')

    expect(get(compileStore).status).toBe('error')
    expect(get(uiStore).status).toContain('stream died')
  })
})
