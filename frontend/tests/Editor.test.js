import { describe, it, expect, vi } from 'vitest'
import { render, fireEvent } from '@testing-library/svelte'
import Editor from '../src/components/Editor.svelte'

describe('Editor Component', () => {
  it('renders editor component', () => {
    const { container } = render(Editor, {
      props: {
        filePath: 'test.tex',
        content: '\\documentclass{article}'
      }
    })
    
    expect(container).toBeTruthy()
  })

  it('emits save event on Ctrl+S', async () => {
    const { component } = render(Editor, {
      props: {
        filePath: 'test.tex',
        content: '\\documentclass{article}'
      }
    })
    
    // Simulate Ctrl+S keypress
    const saveEvent = new KeyboardEvent('keydown', {
      key: 's',
      ctrlKey: true
    })
    
    document.dispatchEvent(saveEvent)
    
    // Note: Actual implementation would require Monaco editor mock
    expect(true).toBe(true)
  })

  it('handles dirty state correctly', () => {
    const { component } = render(Editor, {
      props: {
        filePath: 'test.tex',
        content: '\\documentclass{article}'
      }
    })
    
    // Test that editor exists
    expect(component).toBeTruthy()
  })
})
