// Quick edit from the PDF: the source line behind a spot, edited in a small
// box over the page. Pure helpers; the box itself is QuickEdit.svelte.

/**
 * Offsets of line `line` (1-based) in `text`, without its line ending, or
 * null when the file has no such line. A trailing "\r" belongs to the ending.
 */
export function lineRange(text, line) {
  if (!Number.isInteger(line) || line < 1) return null;
  let start = 0;
  for (let n = 1; n < line; n++) {
    const nl = text.indexOf('\n', start);
    if (nl < 0) return null;
    start = nl + 1;
  }
  let end = text.indexOf('\n', start);
  if (end < 0) end = text.length;
  if (end > start && text[end - 1] === '\r') end -= 1;
  return { start, end };
}

/** The text of line `line`, or null. */
export function lineText(text, line) {
  const r = lineRange(text, line);
  return r ? text.slice(r.start, r.end) : null;
}

/** One line only: a pasted or typed line break would change the file's shape. */
export const singleLine = (value) => String(value).replace(/\r?\n/g, ' ');
