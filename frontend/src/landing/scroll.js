// Where the visitor is on the page, in terms the scene understands: which
// section is at the top of the window and how far through it they are. The
// scene never scrolls the page; the page's own scroll drives the scene.

import { floorPosition } from './scene/story.js';

/** Section rectangles relative to the top of the viewport. */
function measure(elements) {
  return elements
    .filter(Boolean)
    .map((el) => {
      const r = el.getBoundingClientRect();
      return { top: r.top, height: Math.max(1, r.height) };
    });
}

/** The position, 0..n-1 with a fraction for progress through a section. */
function positionOf(elements) {
  return floorPosition(measure(elements));
}

/**
 * Call `onChange(position)` as the page scrolls or resizes, at most once per
 * frame. Returns a function that stops listening.
 */
export function watchScroll(getElements, onChange, win = globalThis.window) {
  let queued = false;
  const run = () => {
    queued = false;
    onChange(positionOf(getElements()));
  };
  const schedule = () => {
    if (queued) return;
    queued = true;
    win.requestAnimationFrame(run);
  };
  win.addEventListener('scroll', schedule, { passive: true });
  win.addEventListener('resize', schedule);
  schedule();
  return () => {
    win.removeEventListener('scroll', schedule);
    win.removeEventListener('resize', schedule);
  };
}

/** Scroll so that `fraction` (0..1) of a pinned section's travel has passed. */
export function scrollToProgress(el, fraction, win = globalThis.window) {
  const travel = Math.max(0, el.offsetHeight - win.innerHeight);
  const top = el.getBoundingClientRect().top + win.scrollY + travel * fraction;
  win.scrollTo({ top, behavior: 'smooth' });
}
