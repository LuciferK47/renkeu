// Coordinate conversion for the visual-fix overlay. Getting this wrong sends
// the backend a box over the wrong part of the page, so it is worth pinning.
import { describe, it, expect } from 'vitest';
import { normalizeDrag, isMeaningfulDrag, toImagePixels, MIN_DRAG_PX } from '../src/utils/bbox.js';

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

describe('toImagePixels', () => {
  const natural = { width: 1275, height: 1650 }; // Letter at 150dpi

  it('is identity at 1:1 zoom', () => {
    const box = { x: 100, y: 200, w: 300, h: 400 };
    expect(toImagePixels(box, natural, natural)).toEqual(box);
  });

  it('scales up when the page is displayed smaller than natural', () => {
    // Displayed at half size: every screen px is two image px.
    const displayed = { width: 637.5, height: 825 };
    expect(toImagePixels({ x: 50, y: 100, w: 150, h: 200 }, displayed, natural)).toEqual({
      x: 100,
      y: 200,
      w: 300,
      h: 400
    });
  });

  it('scales down when zoomed in past natural size', () => {
    const displayed = { width: 2550, height: 3300 }; // 200% zoom
    expect(toImagePixels({ x: 200, y: 400, w: 600, h: 800 }, displayed, natural)).toEqual({
      x: 100,
      y: 200,
      w: 300,
      h: 400
    });
  });

  it('never lets a box run past the image edge', () => {
    const box = { x: 1200, y: 1600, w: 500, h: 500 };
    const region = toImagePixels(box, natural, natural);
    expect(region.x + region.w).toBeLessThanOrEqual(natural.width);
    expect(region.y + region.h).toBeLessThanOrEqual(natural.height);
  });

  it('always sends at least one pixel, so the backend never sees a zero box', () => {
    const region = toImagePixels({ x: 10, y: 10, w: 0, h: 0 }, natural, natural);
    expect(region.w).toBeGreaterThanOrEqual(1);
    expect(region.h).toBeGreaterThanOrEqual(1);
  });

  it('degrades safely before the image has laid out', () => {
    expect(toImagePixels({ x: 1, y: 1, w: 1, h: 1 }, { width: 0, height: 0 }, natural)).toEqual({
      x: 0,
      y: 0,
      w: 0,
      h: 0
    });
  });
});
