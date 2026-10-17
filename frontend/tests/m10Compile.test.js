import { describe, it, expect, beforeEach, vi } from 'vitest'
import { get } from 'svelte/store'

vi.mock('../src/services/compileService.js', () => ({
  startCompilation: vi.fn(),
  streamCompilationStatus: vi.fn(),
  pdfPageUrl: vi.fn((f, n, d, build) => `/api/pdf/page?${build ? `build=${build}` : `file=${f}`}&n=${n}`),
}))
vi.mock('../src/lib/api/client.js', () => ({ api: { cancelCompile: vi.fn(async () => ({})) } }))

import * as compileService from '../src/services/compileService.js'
import { api } from '../src/lib/api/client.js'
import { compileDocument, splitDiagnostics, cancelCompile } from '../src/controllers/CompileController.js'
import { compileStore } from '../src/stores/compileStore.js'

const diagnostics = [
  { severity: 'error', kind: 'undefined_control_sequence', file: 'chapters/a.tex', line: 2, message: 'Undefined control sequence.', context: '\\oops' },
  { severity: 'warning', kind: 'overfull_box', file: 'main.tex', line: 6, message: 'Overfull \\hbox' },
]

/** Make the stream deliver one `compiled` event. */
function streamDelivers(data) {
  compileService.streamCompilationStatus.mockImplementation(async (_id, onEvent) => {
    onEvent({ event: 'compiled', data })
  })
}

describe('splitDiagnostics', () => {
  it('separates errors from warnings and keeps file and line', () => {
    const { errors, warnings } = splitDiagnostics(diagnostics)
    expect(errors).toEqual([
      { message: 'Undefined control sequence.', file: 'chapters/a.tex', line: 2, context: '\\oops', kind: 'undefined_control_sequence' },
    ])
    expect(warnings).toHaveLength(1)
    expect(warnings[0]).toMatchObject({ file: 'main.tex', line: 6 })
  })

  it('tolerates a server that sends nothing', () => {
    expect(splitDiagnostics(undefined)).toEqual({ errors: [], warnings: [] })
    expect(splitDiagnostics(null)).toEqual({ errors: [], warnings: [] })
  })
})

describe('compile results', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    compileStore.update((c) => ({ ...c, status: 'idle', jobId: null, buildId: null, errors: [], warnings: [] }))
    compileService.startCompilation.mockResolvedValue({ job_id: 'job-1', main_file: 'main.tex' })
  })

  it('a failed compile shows located errors and remembers the build', async () => {
    streamDelivers({ success: false, error_summary: 'chapters/a.tex:2: Undefined control sequence.', build_id: 'b'.repeat(32), main_file: 'main.tex', diagnostics })
    await compileDocument('chapters/a.tex')

    const s = get(compileStore)
    expect(s.status).toBe('error')
    expect(s.errors[0]).toMatchObject({ file: 'chapters/a.tex', line: 2 })
    expect(s.buildId).toBe('b'.repeat(32))
    expect(s.mainFile).toBe('main.tex')
  })

  it('falls back to the one-line summary when the server found no locations', async () => {
    streamDelivers({ success: false, error_summary: 'compile cancelled', diagnostics: [] })
    await compileDocument('main.tex')
    expect(get(compileStore).errors).toEqual([{ message: 'compile cancelled' }])
  })

  it('a successful compile points the viewer at the build, so the page URL cannot go stale', async () => {
    const id = 'a'.repeat(32)
    streamDelivers({ success: true, page_count: 3, build_id: id, main_file: 'main.tex', diagnostics: [diagnostics[1]] })
    await compileDocument('main.tex')

    const s = get(compileStore)
    expect(s.status).toBe('success')
    expect(s.pdfUrl).toContain(`build=${id}`)
    expect(s.warnings).toHaveLength(1)
  })
})

describe('cancelCompile', () => {
  it('cancels only a compile that is running', async () => {
    compileStore.update((c) => ({ ...c, status: 'idle', jobId: 'job-9' }))
    expect(await cancelCompile()).toBe(false)
    expect(api.cancelCompile).not.toHaveBeenCalled()

    compileStore.update((c) => ({ ...c, status: 'compiling', jobId: 'job-9' }))
    expect(await cancelCompile()).toBe(true)
    expect(api.cancelCompile).toHaveBeenCalledWith('job-9')
  })
})
