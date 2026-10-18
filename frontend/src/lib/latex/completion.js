// What to offer at a cursor position. Pure: the Monaco provider in register.js
// only translates between this and the editor.

import { COMMANDS, ENVIRONMENTS, PACKAGES, BIB_ENTRY_TYPES } from './snippets.js';

const CITE = /\\(?:[a-zA-Z]*cite[a-zA-Z]*|nocite)\*?(?:\[[^\]]*\]){0,2}\{([^{}]*)$/;
const REF = /\\(?:[a-zA-Z]*ref|eqref|autoref|cref|Cref|nameref|vref)\*?\{([^{}]*)$/;
const INCLUDE = /\\(input|include|subfile)\{([^{}]*)$/;
const GRAPHICS = /\\includegraphics\*?(?:\[[^\]]*\])?\{([^{}]*)$/;
const BIB_LEGACY = /\\bibliography\{([^{}]*)$/;
const BIB_RESOURCE = /\\addbibresource(?:\[[^\]]*\])?\{([^{}]*)$/;
const PACKAGE = /\\(?:usepackage|RequirePackage)(?:\[[^\]]*\])?\{([^{}]*)$/;
const ENV = /\\(begin|end)\{([^{}]*)$/;
const COMMAND = /\\([a-zA-Z@]*)$/;

/** After the cursor, is the current `{…}` group already closed? */
const groupClosed = (rest) => /^[^{}\\]*\}/.test(rest);

/** Prefix of the item being typed in a comma-separated list. */
function listPrefix(group) {
  const at = group.lastIndexOf(',');
  const raw = group.slice(at + 1);
  const trimmed = raw.replace(/^\s+/, '');
  return { prefix: trimmed, lead: raw.length - trimmed.length, at };
}

/**
 * What the text before `offset` is asking for.
 * @param {string} line the whole line
 * @param {number} offset 0-based cursor position in it
 * @returns {null | { kind: string, prefix: string, start: number, end: number, closed: boolean }}
 */
export function contextAt(line, offset) {
  const before = line.slice(0, offset);
  const rest = line.slice(offset);
  const closed = groupClosed(rest);
  const inGroup = (m, group, listy) => {
    if (listy) {
      const { prefix, lead, at } = listPrefix(group);
      return { prefix, start: before.length - group.length + at + 1 + lead, end: offset, closed };
    }
    return { prefix: group, start: before.length - group.length, end: offset, closed };
  };

  let m;
  if ((m = CITE.exec(before))) return { kind: 'cite', ...inGroup(m, m[1], true) };
  if ((m = REF.exec(before))) return { kind: 'ref', ...inGroup(m, m[1], false) };
  if ((m = INCLUDE.exec(before))) return { kind: 'include', ...inGroup(m, m[2], false), command: m[1] };
  if ((m = GRAPHICS.exec(before))) return { kind: 'graphics', ...inGroup(m, m[1], false) };
  if ((m = BIB_LEGACY.exec(before))) return { kind: 'bibfile', legacy: true, ...inGroup(m, m[1], true) };
  if ((m = BIB_RESOURCE.exec(before))) return { kind: 'bibfile', legacy: false, ...inGroup(m, m[1], false) };
  if ((m = PACKAGE.exec(before))) return { kind: 'package', ...inGroup(m, m[1], true) };
  if ((m = ENV.exec(before))) {
    const ctx = { kind: m[1] === 'begin' ? 'env-begin' : 'env-end', ...inGroup(m, m[2], false) };
    // `\begin{|}` with the brace already typed: take it over so the snippet can close it.
    if (ctx.kind === 'env-begin' && rest.startsWith('}')) ctx.end = offset + 1;
    return ctx;
  }
  if ((m = COMMAND.exec(before))) {
    return { kind: 'command', prefix: m[1], start: before.length - m[0].length, end: offset, closed };
  }
  return null;
}

const withClose = (text, ctx) => (ctx.closed ? text : `${text}}`);
const noExt = (p) => p.replace(/\.[^./]+$/, '');
const IMAGE = /\.(png|jpe?g|pdf|svg|eps|gif)$/i;

/**
 * Completion items for a context.
 * @param {ReturnType<typeof contextAt>} ctx
 * @param {{labels: {name:string,path:string,line:number}[], bib: object[], files: string[], macros: string[], environments: string[]}} snap
 * @returns {{label:string, insertText:string, detail?:string, documentation?:string, snippet?:boolean, sortText?:string}[]}
 */
export function suggest(ctx, snap) {
  if (!ctx) return [];
  switch (ctx.kind) {
    case 'cite':
      return snap.bib.map((e) => ({
        label: e.key,
        insertText: withClose(e.key, ctx),
        detail: [e.author, e.year].filter(Boolean).join(' · '),
        documentation: e.title || undefined,
        filterText: `${e.key} ${e.author} ${e.title}`
      }));
    case 'ref':
      return snap.labels.map((l) => ({
        label: l.name,
        insertText: withClose(l.name, ctx),
        detail: `${l.path}:${l.line}`
      }));
    case 'include': {
      const tex = snap.files.filter((f) => /\.tex$/i.test(f));
      return tex.map((f) => ({ label: noExt(f), insertText: withClose(noExt(f), ctx), detail: f }));
    }
    case 'graphics':
      return snap.files
        .filter((f) => IMAGE.test(f))
        .map((f) => ({ label: f, insertText: withClose(f, ctx) }));
    case 'bibfile':
      return snap.files
        .filter((f) => /\.bib$/i.test(f))
        .map((f) => {
          const name = ctx.legacy ? noExt(f) : f;
          return { label: name, insertText: withClose(name, ctx), detail: f };
        });
    case 'package':
      return PACKAGES.map((p) => ({ label: p, insertText: withClose(p, ctx) }));
    case 'env-end':
      return [...new Set([...snap.environments, ...ENVIRONMENTS])].map((n) => ({
        label: n,
        insertText: withClose(n, ctx)
      }));
    case 'env-begin':
      return [...new Set([...snap.environments, ...ENVIRONMENTS])].map((n) => ({
        label: n,
        // Only expand to a full block when this group has no closing brace of
        // its own to disturb (end was moved past a lone `}`, or none follows).
        insertText: ctx.closed && ctx.end === ctx.start + ctx.prefix.length
          ? n
          : `${n}}\n\t$0\n\\end{${n}}`,
        snippet: !(ctx.closed && ctx.end === ctx.start + ctx.prefix.length),
        detail: 'environment'
      }));
    case 'command':
      return [
        ...COMMANDS.map(([label, insertText, detail]) => ({ label, insertText, detail, snippet: true })),
        ...snap.macros.map((m) => ({ label: `\\${m}`, insertText: `\\${m}`, detail: 'defined in this project' }))
      ];
    default:
      return [];
  }
}

/** Completion for `@` in a .bib file. */
export function suggestBib(linePrefix) {
  const m = /(?:^|\s)(@[A-Za-z]*)$/.exec(linePrefix);
  if (!m) return { start: 0, items: [] };
  return {
    start: linePrefix.length - m[1].length,
    items: BIB_ENTRY_TYPES.map(([label, insertText]) => ({ label, insertText, snippet: true, detail: 'entry' }))
  };
}
