import { describe, it, expect, beforeEach } from 'vitest'
import { get } from 'svelte/store'
import { paperStore, activeTone, DEFAULT_TONE } from '../src/stores/paperStore.js'

describe('paperStore', () => {
  beforeEach(() => {
    localStorage.clear()
    paperStore.reset()
  })

  it('starts at the documented default', () => {
    const s = get(paperStore)
    expect(s.paper).toBe(DEFAULT_TONE.paper)
    expect(s.warmth).toBe(DEFAULT_TONE.warmth)
  })

  it('clamps values into the range the server accepts', () => {
    paperStore.setPaper(9)
    expect(get(paperStore).paper).toBe(1)
    paperStore.setPaper(-3)
    expect(get(paperStore).paper).toBe(0.6)
    paperStore.setWarmth(999)
    expect(get(paperStore).warmth).toBe(40)
    paperStore.setWarmth(-1)
    expect(get(paperStore).warmth).toBe(0)
  })

  it('ignores values that are not numbers rather than rendering NaN', () => {
    paperStore.setPaper('not a number')
    expect(get(paperStore).paper).toBe(DEFAULT_TONE.paper)
  })

  it('accepts the strings a range input actually emits', () => {
    paperStore.setPaper('0.75')
    expect(get(paperStore).paper).toBe(0.75)
    paperStore.setWarmth('25')
    expect(get(paperStore).warmth).toBe(25)
  })

  it('persists across a reload', () => {
    paperStore.setPaper(0.72)
    expect(JSON.parse(localStorage.getItem('paper_tone')).paper).toBe(0.72)
  })

  describe('hold to reveal', () => {
    it('asks for the untouched page while held', () => {
      paperStore.setPaper(0.7)
      paperStore.startReveal()
      // paper === 1 is the identity on the server: the page exactly as printed.
      expect(get(activeTone)).toEqual({ paper: 1, warmth: 0 })
    })

    it('restores the reader setting on release rather than discarding it', () => {
      paperStore.setPaper(0.7)
      paperStore.setWarmth(30)
      paperStore.startReveal()
      paperStore.endReveal()
      expect(get(activeTone)).toEqual({ paper: 0.7, warmth: 30 })
    })

    it('survives a release that never had a matching press', () => {
      paperStore.endReveal()
      expect(get(activeTone).paper).toBe(DEFAULT_TONE.paper)
    })
  })
})
