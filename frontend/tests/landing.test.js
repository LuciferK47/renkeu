import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/svelte'

vi.mock('../src/lib/api/client.js', () => ({ api: { getConfig: vi.fn() }, APIError: class extends Error {} }))

import { api } from '../src/lib/api/client.js'
import { EngineStatus } from '../src/landing/engine.svelte.js'
import { watchScroll, scrollToProgress } from '../src/landing/scroll.js'
import Landing from '../src/landing/Landing.svelte'

beforeEach(() => {
  vi.clearAllMocks()
  localStorage.clear()
  // jsdom has no canvas; say so quietly, as a browser without WebGL would.
  HTMLCanvasElement.prototype.getContext = () => null
})
afterEach(() => vi.useRealTimers())

describe('EngineStatus', () => {
  it('is online, with a measured delay, when the server answers', async () => {
    api.getConfig.mockResolvedValue({ ai_enabled: false })
    const status = new EngineStatus({ api })
    expect(status.state).toBe('checking')
    await status.check()
    expect(status.state).toBe('online')
    expect(status.latency).toBeGreaterThanOrEqual(1)
  })

  it('is offline, with no delay, when it does not', async () => {
    api.getConfig.mockRejectedValue(new Error('refused'))
    const status = new EngineStatus({ api })
    await status.check()
    expect(status.state).toBe('offline')
    expect(status.latency).toBeNull()
  })

  it('keeps asking until stopped', async () => {
    vi.useFakeTimers()
    api.getConfig.mockResolvedValue({})
    const status = new EngineStatus({ api })
    const stop = status.start(1000)
    await vi.advanceTimersByTimeAsync(3100)
    expect(api.getConfig.mock.calls.length).toBeGreaterThanOrEqual(4)
    stop()
    const calls = api.getConfig.mock.calls.length
    await vi.advanceTimersByTimeAsync(5000)
    expect(api.getConfig.mock.calls.length).toBe(calls)
  })
})

describe('scroll tracking', () => {
  const el = (top, height) => ({ getBoundingClientRect: () => ({ top, height }) })

  function fakeWindow() {
    const listeners = {}
    return {
      listeners,
      innerHeight: 800,
      scrollY: 0,
      addEventListener: (type, fn) => (listeners[type] = fn),
      removeEventListener: (type) => delete listeners[type],
      requestAnimationFrame: (fn) => fn(),
      scrollTo: vi.fn()
    }
  }

  it('reports the section at the top of the window and how far through it', () => {
    const win = fakeWindow()
    const seen = []
    // The first section is half scrolled past; the second is below.
    const stop = watchScroll(() => [el(-500, 1000), el(500, 1000)], (p) => seen.push(p), win)
    expect(seen.at(-1)).toBe(0.5)
    expect(Object.keys(win.listeners).sort()).toEqual(['resize', 'scroll'])
    stop()
    expect(win.listeners).toEqual({})
  })

  it('scrolls to a fraction of a pinned section\'s travel', () => {
    const win = fakeWindow()
    win.scrollY = 100
    const section = { offsetHeight: 2800, getBoundingClientRect: () => ({ top: 300 }) }
    scrollToProgress(section, 0.5, win)
    // travel = 2800 - 800 = 2000; top = 300 + 100 + 1000
    expect(win.scrollTo).toHaveBeenCalledWith({ top: 1400, behavior: 'smooth' })
  })
})

describe('Landing', () => {
  it('draws every section with no WebGL, and offers to get the app when no engine answers', async () => {
    api.getConfig.mockRejectedValue(new Error('refused'))
    const { container } = render(Landing, { props: { onopenapp: vi.fn() } })

    expect(screen.getByRole('heading', { level: 1 }).textContent).toContain('Write LaTeX.')
    for (const id of ['hero', 'loop', 'workspace', 'engine', 'boundary', 'install', 'faq', 'closing']) {
      expect(container.querySelector(`#${id}`), id).not.toBeNull()
    }
    // jsdom has no WebGL, so the page falls back to still frames: no canvas.
    expect(container.querySelector('.scene canvas')).toBeNull()
    expect(container.querySelector('.landing').dataset.tier).toBe('poster')

    await waitFor(() => expect(container.querySelector('.chip').textContent).toMatch(/Engine offline/))
    expect(screen.queryByRole('button', { name: 'Open editor' })).toBeNull()
    expect(screen.getAllByRole('link', { name: /Get Renkeu/ }).length).toBeGreaterThan(0)
  }, 30_000)

  it('offers to open the editor, and calls back, when an engine answers', async () => {
    api.getConfig.mockResolvedValue({})
    const onopenapp = vi.fn()
    render(Landing, { props: { onopenapp } })
    const buttons = await screen.findAllByRole('button', { name: /Open editor/ })
    buttons[0].click()
    expect(onopenapp).toHaveBeenCalled()
  })

  it('keeps the review gate shut and says so until Apply is pressed', async () => {
    api.getConfig.mockResolvedValue({})
    render(Landing, { props: {} })
    // Step 1 is showing; the review controls belong to step 4.
    expect(screen.queryByRole('button', { name: 'Apply 1 change' })).toBeNull()
    expect(screen.getByText('The compiler says where.')).toBeTruthy()
  })

  it('lists what would cross the boundary, in the words the code uses', () => {
    api.getConfig.mockResolvedValue({})
    render(Landing, { props: {} })
    expect(screen.getAllByText(/up to 3 other files the compiler errors point to/).length).toBeGreaterThan(0)
    expect(screen.getAllByText(/the names of your other files/).length).toBeGreaterThan(0)
  })
})
