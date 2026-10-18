import { describe, it, expect } from 'vitest'
import { splitDiagnostics } from '../src/lib/diagnostics.js'

const diagnostics = [
  { severity: 'error', kind: 'undefined_control_sequence', file: 'chapters/a.tex', line: 2, message: 'Undefined control sequence.', context: '\\oops' },
  { severity: 'warning', kind: 'overfull_box', file: 'main.tex', line: 6, message: 'Overfull \\hbox' },
]

describe('splitDiagnostics', () => {
  it('separates errors from warnings and keeps file, line and context', () => {
    const { errors, warnings } = splitDiagnostics(diagnostics)
    expect(errors).toEqual([
      { message: 'Undefined control sequence.', file: 'chapters/a.tex', line: 2, context: '\\oops', kind: 'undefined_control_sequence' },
    ])
    expect(warnings).toHaveLength(1)
    expect(warnings[0]).toMatchObject({ file: 'main.tex', line: 6, context: null })
  })

  it('tolerates a server that sends nothing', () => {
    expect(splitDiagnostics(undefined)).toEqual({ errors: [], warnings: [] })
    expect(splitDiagnostics(null)).toEqual({ errors: [], warnings: [] })
    expect(splitDiagnostics('garbage')).toEqual({ errors: [], warnings: [] })
  })

  it('a problem with no location is still listed', () => {
    const { errors } = splitDiagnostics([{ severity: 'error', kind: 'fatal', message: 'engine failed' }])
    expect(errors).toEqual([{ message: 'engine failed', file: null, line: null, context: null, kind: 'fatal' }])
  })
})
