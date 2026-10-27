// Coordinate conversion for the visual-fix overlay. Getting this wrong sends
// the backend a box over the wrong part of the page, so it is worth pinning.
import { describe, it, expect } from 'vitest';
import { normalizeDrag, isMeaningfulDrag, toPagePoints, fromPagePoints, MIN_DRAG_PX } from '../src/utils/bbox.js';

const bounds = { width: 800, height: 1000 };

describe('normalizeDrag', () => {
  it('produces a positive-area box dragging down-right', () => {
    expect(normalizeDrag({ x: 10, y: 20 }, { x: 110, y: 220 }, bounds)).toEqual({
      x: 10,
      y: 20,
      w: 100,
      h: 200
    });
  });

  it('handles dragging up-left identically', () => {
    expect(normalizeDrag({ x: 110, y: 220 }, { x: 10, y: 20 }, bounds)).toEqual({
      x: 10,
      y: 20,
      w: 100,
      h: 200
    });
  });

  it('clamps a drag that leaves the image', () => {
    const box = normalizeDrag({ x: 700, y: 900 }, { x: 5000, y: -300 }, bounds);
    expect(box).toEqual({ x: 700, y: 0, w: 100, h: 900 });
  });

  it('yields a zero-area box for a click', () => {
    expect(normalizeDrag({ x: 40, y: 40 }, { x: 40, y: 40 }, bounds)).toEqual({
      x: 40,
      y: 40,
      w: 0,
      h: 0
    });
  });
});

describe('isMeaningfulDrag', () => {
  it('rejects clicks and tiny slips', () => {
    expect(isMeaningfulDrag(null)).toBe(false);
    expect(isMeaningfulDrag({ x: 0, y: 0, w: 0, h: 0 })).toBe(false);
    expect(isMeaningfulDrag({ x: 0, y: 0, w: MIN_DRAG_PX - 1, h: 50 })).toBe(false);
    expect(isMeaningfulDrag({ x: 0, y: 0, w: 50, h: MIN_DRAG_PX - 1 })).toBe(false);
  });

  it('accepts a real selection', () => {
    expect(isMeaningfulDrag({ x: 0, y: 0, w: MIN_DRAG_PX, h: MIN_DRAG_PX })).toBe(true);
  });
});

describe('toPagePoints', () => {
  it('divides screen px by px-per-point, so zoom does not change the answer', () => {
    // The same physical box at 100% (96/72 px per pt) and at 200%.
    const at100 = toPagePoints({ x: 133.333, y: 66.667, w: 266.667, h: 133.333 }, 96 / 72);
    const at200 = toPagePoints({ x: 266.667, y: 133.333, w: 533.333, h: 266.667 }, 192 / 72);
    expect(at100).toEqual({ x: 100, y: 50, w: 200, h: 100 });
    expect(at200).toEqual(at100);
  });

  it('refuses a nonsensical scale rather than dividing by zero', () => {
    for (const bad of [0, -1, NaN, undefined]) {
      expect(toPagePoints({ x: 1, y: 1, w: 1, h: 1 }, bad)).toEqual({ x: 0, y: 0, w: 0, h: 0 });
    }
  });

  it('round-trips with fromPagePoints', () => {
    const region = { x: 72, y: 144, w: 36, h: 18 };
    expect(toPagePoints(fromPagePoints(region, 1.5), 1.5)).toEqual(region);
  });
});
