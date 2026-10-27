// The front-matter block of a document made from a template: a region of
// main.tex between two marker comments, whose first line records the template
// and the values (as JSON) so a form can edit them without parsing LaTeX. The
// server renders the block; this only finds it.

const BEGIN = '% renkeu:frontmatter begin';
const END = '% renkeu:frontmatter end';

export const emptyFields = () => ({
  title: '',
  authors: [{ name: '', affiliation: '', email: '' }],
  abstract: '',
  keywords: '',
  date: ''
});

/**
 * Find the block in `text`: `{ template, fields, start, end }` with character
 * offsets covering both marker lines, or null when there is none (a document
 * written by hand, or a block whose record was damaged).
 */
export function findBlock(text) {
  const start = text.indexOf(BEGIN);
  if (start < 0) return null;
  const endMarker = text.indexOf(END, start);
  if (endMarker < 0) return null;
  const lineEnd = text.indexOf('\n', start);
  const line = text.slice(start, lineEnd < 0 ? text.length : lineEnd);
  const m = /^% renkeu:frontmatter begin template=(\S+) fields=(.*)$/.exec(line);
  if (!m) return null;
  let fields;
  try {
    fields = JSON.parse(m[2]);
  } catch {
    return null;
  }
  return { template: m[1], fields: { ...emptyFields(), ...fields }, start, end: endMarker + END.length };
}
