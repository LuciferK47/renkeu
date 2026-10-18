import { describe, it, expect, afterEach, vi } from 'vitest'
import { isDesktop, pickProject } from '../src/lib/desktop.js'

afterEach(() => { delete globalThis.__TAURI_INTERNALS__ })

describe('desktop hooks', () => {
  it('are absent in a browser, and picking says so rather than failing obscurely', async () => {
    expect(isDesktop()).toBe(false)
    await expect(pickProject()).rejects.toThrow(/desktop app/)
  })

  it('call the shell command when it is there', async () => {
    const invoke = vi.fn(async () => false)
    globalThis.__TAURI_INTERNALS__ = { invoke }
    expect(isDesktop()).toBe(true)
    expect(await pickProject()).toBe(false)
    expect(invoke).toHaveBeenCalledWith('pick_project')
  })
})
