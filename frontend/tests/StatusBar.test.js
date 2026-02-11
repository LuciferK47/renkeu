import { describe, it, expect } from 'vitest'
import { render, fireEvent, waitFor } from '@testing-library/svelte'
import StatusBar from '../src/components/StatusBar.svelte'

describe('StatusBar Component', () => {
  it('displays status message', () => {
    const { getByText } = render(StatusBar, {
      props: {
        status: 'Ready',
        compiling: false
      }
    })
    
    expect(getByText('Ready')).toBeTruthy()
  })

  it('shows compiling state', () => {
    const { container } = render(StatusBar, {
      props: {
        status: 'Compiling...',
        compiling: true
      }
    })
    
    expect(container.textContent).toContain('Compiling')
  })

  it('displays file path when provided', () => {
    const { getByText } = render(StatusBar, {
      props: {
        status: 'Ready',
        filePath: 'test.tex',
        compiling: false
      }
    })
    
    expect(getByText(/test\.tex/)).toBeTruthy()
  })
})
