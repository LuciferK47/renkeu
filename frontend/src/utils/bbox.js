/**
 * Bounding-box maths for the visual-fix overlay.
 *
 * Three coordinate spaces are involved:
 *  1. screen px  - where the pointer actually is, relative to the <img>;
 *  2. image px   - the rendered page PNG's natural size (what the backend
 *                  expects, because it rasterised at a known DPI);
 *  3. PDF points - the backend's own space; we never touch it here.
 *
 * Zooming changes 1 without changing 2, which is exactly why the conversion
 * has to be explicit.
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
 * Convert a box in displayed/screen px into the page image's own pixel grid.
 * `displayed` is the <img> element's rendered size, `natural` its intrinsic
 * size — their ratio is the zoom factor.
 */
export function toImagePixels(box, displayed, natural) {
  if (!displayed?.width || !displayed?.height) return { x: 0, y: 0, w: 0, h: 0 };

  const scaleX = natural.width / displayed.width;
  const scaleY = natural.height / displayed.height;

  const x = Math.round(box.x * scaleX);
  const y = Math.round(box.y * scaleY);

  return {
    // Clamp so a drag ending on the border cannot exceed the image.
    x: Math.max(0, Math.min(x, natural.width - 1)),
    y: Math.max(0, Math.min(y, natural.height - 1)),
    w: Math.max(1, Math.min(Math.round(box.w * scaleX), natural.width - x)),
    h: Math.max(1, Math.min(Math.round(box.h * scaleY), natural.height - y))
  };
}
