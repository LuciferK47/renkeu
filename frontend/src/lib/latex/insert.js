// Builders for the Insert menu: figures, tables, equations, citations and
// cross-references as LaTeX text, plus the small helpers they share. Pure, so
// each can be tested without a browser.

import { scan } from './scan.js';
import { scanBib } from './bibtex.js';

/** Escape text for a LaTeX argument (mirrors `escape_latex` in the server). */
export function escapeLatex(text) {
  return String(text)
    .replace(/\r/g, '')
    .replace(/[\\~^&%$#_{}]/g, (c) => {
      if (c === '\\') return '\\textbackslash{}';
      if (c === '~') return '\\textasciitilde{}';
      if (c === '^') return '\\textasciicircum{}';
      return `\\${c}`;
    });
}

/** `fig:my-caption` from a prefix and free text; never empty. */
export function labelFrom(prefix, text) {
  const slug = String(text)
    .normalize('NFD')
    .replace(/[̀-ͯ]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 32)
    .replace(/-+$/g, '');
  return `${prefix}:${slug || 'untitled'}`;
}

/** `label`, or `label-2`, `label-3`… so it clashes with nothing in `taken`. */
export function uniqueLabel(label, taken) {
  const used = new Set(taken);
  if (!used.has(label)) return label;
  for (let n = 2; ; n++) if (!used.has(`${label}-${n}`)) return `${label}-${n}`;
}

const PLACEMENTS = new Set(['h', 't', 'b', 'p', 'H', '!']);

/** A `figure` environment around `\includegraphics`. */
export function figureBlock({ path, caption = '', label = '', widthPct = 80, placement = 'htbp' }) {
  const width = Math.min(100, Math.max(5, Math.round(Number(widthPct) || 80)));
  const place = [...String(placement)].filter((c) => PLACEMENTS.has(c)).join('') || 'htbp';
  const lines = [
    `\\begin{figure}[${place}]`,
    '  \\centering',
    `  \\includegraphics[width=${(width / 100).toFixed(2).replace(/\.?0+$/, '')}\\linewidth]{${path}}`
  ];
  if (caption.trim()) lines.push(`  \\caption{${escapeLatex(caption.trim())}}`);
  if (label.trim()) lines.push(`  \\label{${label.trim()}}`);
  lines.push('\\end{figure}');
  return lines.join('\n');
}

/**
 * A `table` with a booktabs `tabular`. `rows` is an array of arrays of
 * strings; `align` has one of l/c/r per column. Cells are escaped unless
 * `rawCells` (the user typed LaTeX, such as `$x^2$`).
 */
export function tableBlock({ rows, header = true, align = [], caption = '', label = '', rawCells = false }) {
  const cols = Math.max(1, ...rows.map((r) => r.length));
  const spec = Array.from({ length: cols }, (_, i) => (['l', 'c', 'r'].includes(align[i]) ? align[i] : 'l')).join('');
  const cell = (v) => (rawCells ? String(v ?? '').trim() : escapeLatex(String(v ?? '').trim()));
  const line = (r) => `    ${Array.from({ length: cols }, (_, i) => cell(r[i])).join(' & ')} \\\\`;
  const out = ['\\begin{table}[htbp]', '  \\centering'];
  if (caption.trim()) out.push(`  \\caption{${escapeLatex(caption.trim())}}`);
  if (label.trim()) out.push(`  \\label{${label.trim()}}`);
  out.push(`  \\begin{tabular}{${spec}}`, '    \\toprule');
  rows.forEach((r, i) => {
    out.push(line(r));
    if (header && i === 0 && rows.length > 1) out.push('    \\midrule');
  });
  out.push('    \\bottomrule', '  \\end{tabular}', '\\end{table}');
  return out.join('\n');
}

/** Rows of cells from text pasted out of a spreadsheet (tabs) or a CSV-ish list. */
export function parseGrid(text) {
  const lines = String(text).replace(/\r/g, '').split('\n').filter((l) => l.trim() !== '');
  const sep = lines.some((l) => l.includes('\t')) ? '\t' : ',';
  return lines.map((l) => l.split(sep).map((c) => c.trim()));
}

/** Maths as inline, display, or a numbered equation. */
export function equationBlock({ latex, mode = 'display', label = '' }) {
  const body = String(latex).trim();
  if (mode === 'inline') return `$${body}$`;
  if (mode === 'numbered') {
    const lab = label.trim() ? `\n  \\label{${label.trim()}}` : '';
    return `\\begin{equation}${lab}\n  ${body}\n\\end{equation}`;
  }
  return `\\[\n  ${body}\n\\]`;
}

/**
 * Where and how to put a block (a figure, table or display equation) so it
 * never splits a line and sits between blank lines. `offset` is the cursor.
 * A blank line, or a cursor at the start of a line, takes the block above that
 * line; a cursor inside text puts it after the line, since a block cannot
 * start in the middle of a sentence. Returns `{ offset, text }` to insert.
 */
export function blockInsertion(doc, offset, block) {
  const lineStart = doc.lastIndexOf('\n', offset - 1) + 1;
  let lineEnd = doc.indexOf('\n', offset);
  if (lineEnd < 0) lineEnd = doc.length;
  const blank = doc.slice(lineStart, lineEnd).trim() === '';
  const lineAt = (from) => {
    const end = doc.indexOf('\n', from);
    return doc.slice(from, end < 0 ? doc.length : end);
  };
  if (blank || offset === lineStart) {
    const prevBlank = lineStart === 0 || lineAt(doc.lastIndexOf('\n', lineStart - 2) + 1).trim() === '';
    return { offset: lineStart, text: `${prevBlank ? '' : '\n'}${block}\n${blank ? '' : '\n'}` };
  }
  const next = lineEnd < doc.length ? lineAt(lineEnd + 1) : '';
  return { offset: lineEnd, text: `\n\n${block}${next.trim() === '' ? '' : '\n'}` };
}

export const citeCommand = (cmd, keys) => `\\${cmd}{${keys.join(',')}}`;
export const refCommand = (cmd, label) => `\\${cmd}{${label}}`;

/** Packages that provide commands people use without loading them. */
const COMMAND_PACKAGES = {
  includegraphics: 'graphicx',
  graphicspath: 'graphicx',
  toprule: 'booktabs',
  midrule: 'booktabs',
  bottomrule: 'booktabs',
  cmidrule: 'booktabs',
  SI: 'siunitx',
  si: 'siunitx',
  num: 'siunitx',
  qty: 'siunitx',
  ang: 'siunitx',
  cref: 'cleveref',
  Cref: 'cleveref',
  autoref: 'hyperref',
  url: 'hyperref',
  href: 'hyperref',
  mathbb: 'amssymb',
  mathfrak: 'amssymb',
  text: 'amsmath',
  dfrac: 'amsmath',
  tfrac: 'amsmath',
  eqref: 'amsmath',
  textcolor: 'xcolor',
  colorbox: 'xcolor',
  xspace: 'xspace',
  citep: 'natbib',
  citet: 'natbib',
  bm: 'bm',
  multirow: 'multirow',
  subcaption: 'subcaption'
};

/** The package that defines `\name`, if it is one of the well-known ones. */
export const packageForCommand = (name) => COMMAND_PACKAGES[name] ?? null;

/**
 * The edit that adds `\usepackage{…}` lines for the packages `text` lacks:
 * `{ offset, text }`, or null when nothing is missing or there is nowhere
 * sensible to put them (no `\documentclass`).
 */
export function packageEdit(text, packages) {
  const have = new Set(scan(text).packages);
  const missing = [...new Set(packages)].filter((p) => !have.has(p));
  if (missing.length === 0) return null;
  const add = missing.map((p) => `\\usepackage{${p}}\n`).join('');
  const begin = /^[ \t]*\\begin\{document\}/m.exec(text);
  if (begin) return { offset: begin.index, text: add };
  const cls = /^[^%\n]*\\documentclass(?:\[[^\]]*\])?\{[^}]*\}[^\n]*\n?/m.exec(text);
  if (cls) return { offset: cls.index + cls[0].length, text: cls[0].endsWith('\n') ? add : `\n${add}` };
  return null;
}

/**
 * The quick fix for a compile error on one line, or null: an "Undefined
 * control sequence" whose command belongs to a well-known package that is not
 * loaded. Returns `{ command, package, edit: { offset, text } }`.
 */
export function packageFixFor({ message, line, text }) {
  if (!/^Undefined control sequence/.test(message)) return null;
  for (const m of line.matchAll(/\\([A-Za-z]+)/g)) {
    const pkg = packageForCommand(m[1]);
    const edit = pkg && packageEdit(text, [pkg]);
    if (edit) return { command: m[1], package: pkg, edit };
  }
  return null;
}

// ---- BibTeX from a DOI --------------------------------------------------

const STOP = new Set(['the', 'a', 'an', 'of', 'on', 'for', 'and', 'with', 'from', 'into', 'in', 'to', 'by']);
const ascii = (s) => s.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase().replace(/[^a-z0-9]/g, '');

/** `knuth1984literate` from an entry's author, year and title. */
function keyFor(entry) {
  const first = (entry.author || '').split(/\s+and\s+/i)[0].trim();
  const surname = first.includes(',') ? first.split(',')[0] : first.split(/\s+/).at(-1) ?? '';
  const word = (entry.title || '').split(/\s+/).map(ascii).find((w) => w.length > 3 && !STOP.has(w)) ?? '';
  return `${ascii(surname) || 'ref'}${entry.year || ''}${word}`;
}

/**
 * Tidy the BibTeX a DOI service returns: a readable key (`knuth1984literate`,
 * made unique among `taken`), nothing else changed. Returns `{ key, text }`.
 */
export function cleanBibtex(bibtex, taken = []) {
  const text = bibtex.trim();
  const entry = scanBib(text)[0];
  if (!entry) return { key: '', text };
  const used = new Set(taken);
  const base = keyFor(entry);
  let key = base;
  for (let n = 0; used.has(key); n++) key = `${base}${String.fromCharCode(97 + (n % 26))}${n >= 26 ? n : ''}`;
  return { key, text: text.replace(/^(@[A-Za-z]+\s*\{)\s*[^,\s]+,/, `$1${key},`) + '\n' };
}
