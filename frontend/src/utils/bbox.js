/**
 * Bounding-box maths for the visual-fix overlay.
 *
 * Two coordinate spaces are involved:
 *  1. screen px  - where the pointer is, relative to the page on screen;
 *  2. PDF points - 1/72 inch from the page's top-left, the unit SyncTeX and the
 *                  backend use. It does not change with zoom, device pixel
 *                  ratio or the DPI a page image was rendered at.
 *
 * Zoom changes the first without changing the second, so the conversion is
 * explicit and every box sent to the server is in points.
 */

/** Smallest drag (screen px) treated as a box rather than a stray click. */
export const MIN_DRAG_PX = 8;

/**
 * Normalise a drag into a positive-area box clamped to `bounds`,
 * so dragging up-left works the same as down-right.
 */
export function normalizeDrag(start, current, bounds) {
  const clamp = (v, max) => Math.max(0, Math.min(v, max));
  const x1 = clamp(start.x, bounds.width);
  const y1 = clamp(start.y, bounds.height);
  const x2 = clamp(current.x, bounds.width);
  const y2 = clamp(current.y, bounds.height);

  return {
    x: Math.min(x1, x2),
    y: Math.min(y1, y2),
    w: Math.abs(x2 - x1),
    h: Math.abs(y2 - y1)
  };
}

/** Is this drag big enough to mean something? */
export function isMeaningfulDrag(box) {
  return !!box && box.w >= MIN_DRAG_PX && box.h >= MIN_DRAG_PX;
}

/**
 * Convert a box in screen px (relative to the page) to PDF points.
 * `pxPerPt` is how many screen px one point occupies at the current zoom.
 * Rounded to a tenth of a point: far finer than any glyph.
 */
export function toPagePoints(box, pxPerPt) {
  if (!(pxPerPt > 0)) return { x: 0, y: 0, w: 0, h: 0 };
  const pt = (v) => Math.round((v / pxPerPt) * 10) / 10;
  return { x: pt(box.x), y: pt(box.y), w: pt(box.w), h: pt(box.h) };
}

/** The inverse: a region in points as screen px, for drawing it. */
export function fromPagePoints(region, pxPerPt) {
  return { x: region.x * pxPerPt, y: region.y * pxPerPt, w: region.w * pxPerPt, h: region.h * pxPerPt };
}
