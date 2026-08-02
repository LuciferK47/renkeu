import { writable, derived } from 'svelte/store'

// How the compiled page is toned for reading on a dark ground.
//
// On by default: the whole point is that a white page beside a dark editor is
// what drives people to close the preview at night. Adjustable because the
// contrast-polarity research (arXiv:2404.11454) found the comfortable setting
// varies per person, so a fixed value would be wrong for a real share of users.
//
// Lightness 1 is the identity: the server sends the page exactly as it prints.

const STORAGE_KEY = 'paper_tone'

export const DEFAULT_TONE = { paper: 0.88, warmth: 18 }

function load() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return { ...DEFAULT_TONE }
    const saved = JSON.parse(raw)
    return {
      paper: clamp(Number(saved.paper), 0.6, 1, DEFAULT_TONE.paper),
      warmth: clamp(Number(saved.warmth), 0, 40, DEFAULT_TONE.warmth),
    }
  } catch {
    return { ...DEFAULT_TONE }
  }
}

function clamp(v, lo, hi, fallback) {
  return Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : fallback
}

function createPaperStore() {
  const { subscribe, update, set } = writable({ ...load(), revealing: false })

  const persist = (state) => {
    try {
      localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({ paper: state.paper, warmth: state.warmth }),
      )
    } catch {
      // A private-mode browser refusing storage is not worth failing a render.
    }
    return state
  }

  return {
    subscribe,
    setPaper: (paper) =>
      update((s) => persist({ ...s, paper: clamp(Number(paper), 0.6, 1, s.paper) })),
    setWarmth: (warmth) =>
      update((s) => persist({ ...s, warmth: clamp(Number(warmth), 0, 40, s.warmth) })),
    reset: () => set(persist({ ...DEFAULT_TONE, revealing: false })),
    // Held while the reader is checking true colours before submitting.
    startReveal: () => update((s) => ({ ...s, revealing: true })),
    endReveal: () => update((s) => ({ ...s, revealing: false })),
  }
}

export const paperStore = createPaperStore()

// What the page request should actually ask for: revealing overrides the
// setting without discarding it, so releasing the control restores the tone.
export const activeTone = derived(paperStore, ($p) =>
  $p.revealing ? { paper: 1, warmth: 0 } : { paper: $p.paper, warmth: $p.warmth },
)
