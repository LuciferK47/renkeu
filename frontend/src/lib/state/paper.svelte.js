// How the compiled page is toned for reading on a dark ground.
//
// On by default: the whole point is that a white page beside a dark editor is
// what drives people to close the preview at night. Adjustable because the
// contrast-polarity research (arXiv:2404.11454) found the comfortable setting
// varies per person, so a fixed value would be wrong for a real share of users.
//
// Lightness 1 is the identity: the server sends the page exactly as it prints.

import { readPref, writePref } from '../storage.js';

const KEY = 'paper_tone';

export const DEFAULT_TONE = { paper: 0.88, warmth: 18 };

const clamp = (v, lo, hi, fallback) => (Number.isFinite(Number(v)) && v !== null && v !== '' ? Math.min(hi, Math.max(lo, Number(v))) : fallback);

export class PaperState {
  paper = $state(DEFAULT_TONE.paper);
  warmth = $state(DEFAULT_TONE.warmth);
  /** Held while the reader is checking true colours before submitting. */
  revealing = $state(false);

  constructor() {
    const saved = readPref(KEY, null);
    if (saved) {
      this.paper = clamp(saved.paper, 0.6, 1, DEFAULT_TONE.paper);
      this.warmth = clamp(saved.warmth, 0, 40, DEFAULT_TONE.warmth);
    }
  }

  /** What a page request should ask for. Revealing overrides the setting
   *  without discarding it, so releasing the control restores the tone. */
  get tone() {
    return this.revealing ? { paper: 1, warmth: 0 } : { paper: this.paper, warmth: this.warmth };
  }

  setPaper(value) {
    this.paper = clamp(value, 0.6, 1, this.paper);
    this.#save();
  }

  setWarmth(value) {
    this.warmth = clamp(value, 0, 40, this.warmth);
    this.#save();
  }

  reset() {
    this.paper = DEFAULT_TONE.paper;
    this.warmth = DEFAULT_TONE.warmth;
    this.revealing = false;
    this.#save();
  }

  // Arrow functions: these are handed straight to event handlers.
  startReveal = () => {
    this.revealing = true;
  };

  endReveal = () => {
    this.revealing = false;
  };

  #save() {
    writePref(KEY, { paper: this.paper, warmth: this.warmth });
  }
}
