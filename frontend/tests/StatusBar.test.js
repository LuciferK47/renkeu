import { describe, it, expect } from 'vitest'
import { render } from '@testing-library/svelte'
import StatusBar from '../src/components/StatusBar.svelte'

const doc = (over = {}) => ({ dirty: false, saving: false, conflict: null, error: null, ...over })

describe('StatusBar', () => {
  it('shows the status and, for the open file, whether it is saved', () => {
    const { getByText, rerender } = render(StatusBar, { props: { status: 'Ready', doc: doc() } })
    expect(getByText('Ready')).toBeTruthy()
    expect(getByText('Saved')).toBeTruthy()

    rerender({ status: 'Ready', doc: doc({ dirty: true }) })
    expect(getByText('Unsaved changes')).toBeTruthy()
  })

  it('says so plainly when a save failed or the file conflicts', () => {
    const { getByText, rerender } = render(StatusBar, { props: { status: '', doc: doc({ error: 'offline' }) } })
    expect(getByText('Not saved: offline')).toBeTruthy()
    rerender({ status: '', doc: doc({ conflict: { serverEtag: 'x' } }) })
    expect(getByText(/Conflict/)).toBeTruthy()
  })

  it('shows compile progress and the cursor position', () => {
    const { container } = render(StatusBar, {
      props: { status: '', doc: doc(), compile: { compiling: true, progress: 40 }, cursor: { line: 7, column: 3 } }
    })
    expect(container.textContent).toContain('Compiling 40%')
    expect(container.textContent).toContain('Ln 7, Col 3')
  })

  it('shows no save state when nothing is open', () => {
    const { container } = render(StatusBar, { props: { status: 'Ready' } })
    expect(container.textContent).not.toMatch(/Saved|Unsaved/)
  })
})
