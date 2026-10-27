// Following and explaining references: what is under the cursor, where it is
// defined, and what to say about it on hover. Pure; register.js and the editor
// pane connect it to Monaco.

import { COMMANDS } from './snippets.js';

const CITE_CMD = /^(?:[a-zA-Z]*cite[a-zA-Z]*|nocite)$/;
const REF_CMD = /^(?:[a-zA-Z]*ref|eqref|autoref|cref|Cref|nameref|vref)$/;
const GROUP = /\\([a-zA-Z@]+)\*?((?:\[[^\]]*\])*)\{([^{}]*)\}/g;

function classify(command) {
  if (CITE_CMD.test(command)) return { kind: 'cite', list: true };
  if (REF_CMD.test(command)) return { kind: 'ref', list: false };
  if (command === 'input' || command === 'include' || command === 'subfile') return { kind: 'include', list: false };
  if (command === 'includegraphics') return { kind: 'graphics', list: false };
  if (command === 'bibliography') return { kind: 'bib', list: true, legacy: true };
  if (command === 'addbibresource') return { kind: 'bib', list: false, legacy: false };
  return null;
}

/**
 * The thing at a cursor position that can be followed or explained.
 * @param {string} line
 * @param {number} column 1-based, as Monaco reports it
 * @returns {null | { kind: string, name: string, start: number, end: number, legacy?: boolean }}
 *   `start`/`end` are 0-based offsets of the name within the line
 */
export function referenceAt(line, column) {
  const at = column - 1;
  GROUP.lastIndex = 0;
  for (let m = GROUP.exec(line); m; m = GROUP.exec(line)) {
    const kind = classify(m[1]);
    if (!kind) continue;
    const groupStart = m.index + m[0].length - 1 - m[3].length;
    const groupEnd = groupStart + m[3].length;
    if (at < groupStart || at > groupEnd) continue;
    if (!kind.list) {
      const name = m[3].trim();
      return name ? { kind: kind.kind, name, start: groupStart, end: groupEnd, legacy: kind.legacy } : null;
    }
    let pos = groupStart;
    for (const part of m[3].split(',')) {
      const lead = part.length - part.trimStart().length;
      const name = part.trim();
      const start = pos + lead;
      const end = start + name.length;
      if (name && at >= start && at <= end) return { kind: kind.kind, name, start, end, legacy: kind.legacy };
      pos += part.length + 1;
    }
    return null;
  }
  // A control sequence on its own.
  const word = /\\[a-zA-Z@]+/g;
  for (let m = word.exec(line); m; m = word.exec(line)) {
    if (at >= m.index && at <= m.index + m[0].length) {
      return { kind: 'command', name: m[0].slice(1), start: m.index, end: m.index + m[0].length };
    }
  }
  return null;
}

const IMAGE_EXT = ['', '.png', '.pdf', '.jpg', '.jpeg', '.svg', '.eps', '.gif'];

/** Find `target` among project files, trying the extensions TeX would. */
function findFile(target, files, extensions) {
  const clean = target.replace(/^\.\//, '');
  for (const ext of extensions) {
    if (files.includes(clean + ext)) return clean + ext;
  }
  return null;
}

/**
 * Where a reference is defined.
 * @returns {null | { path: string, line: number }}
 */
export function definitionFor(ref, snap) {
  if (!ref) return null;
  switch (ref.kind) {
    case 'cite': {
      const e = snap.bib.find((b) => b.key === ref.name);
      return e ? { path: e.path, line: e.line } : null;
    }
    case 'ref': {
      const l = snap.labels.find((x) => x.name === ref.name);
      return l ? { path: l.path, line: l.line } : null;
    }
    case 'include': {
      const f = findFile(ref.name, snap.files, ['', '.tex']);
      return f ? { path: f, line: 1 } : null;
    }
    case 'graphics': {
      const f = findFile(ref.name, snap.files, IMAGE_EXT);
      return f ? { path: f, line: 1 } : null;
    }
    case 'bib': {
      const f = findFile(ref.name, snap.files, ref.legacy ? ['.bib', ''] : ['', '.bib']);
      return f ? { path: f, line: 1 } : null;
    }
    case 'command': {
      const d = snap.macroDefs.find((m) => m.name === ref.name);
      return d ? { path: d.path, line: d.line } : null;
    }
    default:
      return null;
  }
}

const fence = (s) => `\`${s.replace(/`/g, "'")}\``;

/**
 * Markdown to show when hovering a reference, or null for nothing to say.
 * An unresolved citation or label is worth saying out loud: it prints as "?".
 */
export function hoverFor(ref, snap) {
  if (!ref) return null;
  switch (ref.kind) {
    case 'cite': {
      const e = snap.bib.find((b) => b.key === ref.name);
      if (!e) return `⚠ No bibliography entry named ${fence(ref.name)}. It will print as **[?]**.`;
      const who = [e.author, e.year && `(${e.year})`].filter(Boolean).join(' ');
      return [`**${e.title || e.key}**`, who, `*${e.type}* in ${fence(`${e.path}:${e.line}`)}`].filter(Boolean).join('\n\n');
    }
    case 'ref': {
      const l = snap.labels.find((x) => x.name === ref.name);
      if (!l) return `⚠ No \\label named ${fence(ref.name)}. It will print as **??**.`;
      return `${fence(`${l.path}:${l.line}`)}\n\n${fence(l.text)}`;
    }
    case 'include':
    case 'graphics':
    case 'bib': {
      const def = definitionFor(ref, snap);
      return def ? `${fence(def.path)}` : `⚠ ${fence(ref.name)} was not found in this project.`;
    }
    case 'command': {
      const d = snap.macroDefs.find((m) => m.name === ref.name);
      if (d) return `Defined at ${fence(`${d.path}:${d.line}`)}\n\n${fence(d.text)}`;
      const known = COMMANDS.find(([label]) => label === `\\${ref.name}`);
      return known ? `${fence(known[0])} — ${known[2]}` : null;
    }
    default:
      return null;
  }
}
