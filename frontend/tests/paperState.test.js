import { describe, it, expect, beforeEach } from 'vitest'
import { PaperState, DEFAULT_TONE } from '../src/lib/state/paper.svelte.js'

describe('PaperState', () => {
  let p
  beforeEach(() => {
    localStorage.clear()
    p = new PaperState()
  })

  it('starts at the documented default', () => {
    expect(p.paper).toBe(DEFAULT_TONE.paper)
    expect(p.warmth).toBe(DEFAULT_TONE.warmth)
  })

  it('clamps values into the range the server accepts', () => {
    p.setPaper(9); expect(p.paper).toBe(1)
    p.setPaper(-3); expect(p.paper).toBe(0.6)
    p.setWarmth(999); expect(p.warmth).toBe(40)
    p.setWarmth(-1); expect(p.warmth).toBe(0)
  })

  it('ignores values that are not numbers rather than rendering NaN', () => {
    p.setPaper('not a number'); expect(p.paper).toBe(DEFAULT_TONE.paper)
    p.setWarmth(''); expect(p.warmth).toBe(DEFAULT_TONE.warmth)
    p.setPaper(null); expect(p.paper).toBe(DEFAULT_TONE.paper)
  })

  it('accepts the strings a range input actually emits', () => {
    p.setPaper('0.75'); expect(p.paper).toBe(0.75)
    p.setWarmth('25'); expect(p.warmth).toBe(25)
  })

  it('persists, and a new instance picks it up (a reload)', () => {
    p.setPaper(0.72)
    p.setWarmth(11)
    const again = new PaperState()
    expect([again.paper, again.warmth]).toEqual([0.72, 11])
  })

  it('a corrupt saved value falls back to the default instead of breaking the page', () => {
    localStorage.setItem('paper_tone', JSON.stringify({ paper: 'x', warmth: 9999 }))
    const q = new PaperState()
    expect(q.paper).toBe(DEFAULT_TONE.paper)
    expect(q.warmth).toBe(40)
  })

  it('reset restores the default and saves it', () => {
    p.setPaper(0.7)
    p.reset()
    expect(new PaperState().paper).toBe(DEFAULT_TONE.paper)
  })

  describe('hold to reveal', () => {
    it('asks for the untouched page while held', () => {
      p.setPaper(0.7)
      p.startReveal()
      // paper === 1 is the identity on the server: the page exactly as printed.
      expect(p.tone).toEqual({ paper: 1, warmth: 0 })
    })

    it('restores the reader setting on release rather than discarding it', () => {
      p.setPaper(0.7); p.setWarmth(30)
      p.startReveal(); p.endReveal()
      expect(p.tone).toEqual({ paper: 0.7, warmth: 30 })
    })

    it('survives a release that never had a matching press', () => {
      p.endReveal()
      expect(p.tone.paper).toBe(DEFAULT_TONE.paper)
    })

    it('does not persist the reveal, only the setting', () => {
      p.startReveal()
      expect(new PaperState().revealing).toBe(false)
    })
  })
})
