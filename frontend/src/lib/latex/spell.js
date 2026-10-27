// Spell checking for the prose in a LaTeX file. The hard part is deciding what
// *is* prose: commands, maths, comments, verbatim blocks and the arguments of
// \cite, \ref, \usepackage and friends are all blanked out first, so the
// checker sees only what a reader would read.

import { stripComment, balancedGroup } from './scan.js';

const BLANK_ENVS = [
  'verbatim', 'lstlisting', 'minted', 'comment', 'equation', 'align', 'gather', 'multline', 'flalign',
  'eqnarray', 'math', 'displaymath', 'split', 'array', 'tikzpicture', 'tabular', 'matrix', 'pmatrix',
  'bmatrix', 'cases'
];

// Commands whose arguments are names or code, not prose.
const CODE_ARGS = new Set([
  'cite', 'citep', 'citet', 'citeauthor', 'citeyear', 'parencite', 'textcite', 'autocite', 'nocite',
  'ref', 'eqref', 'pageref', 'autoref', 'cref', 'Cref', 'nameref', 'vref', 'label', 'input', 'include',
  'subfile', 'usepackage', 'RequirePackage', 'documentclass', 'includegraphics', 'bibliography',
  'bibliographystyle', 'addbibresource', 'begin', 'end', 'url', 'href', 'newcommand', 'renewcommand',
  'providecommand', 'newenvironment', 'renewenvironment', 'def', 'let', 'DeclareMathOperator',
  'newtheorem', 'setlength', 'geometry', 'hypersetup', 'lstset', 'pagestyle', 'thispagestyle',
  'bibitem', 'hyperref', 'texorpdfstring', 'newcounter', 'setcounter', 'addtocounter', 'color', 'textcolor'
]);

// Brace groups swallowed after a command (default 1).
const GROUP_COUNT = {
  newcommand: 2, renewcommand: 2, providecommand: 2, DeclareMathOperator: 2, newenvironment: 3,
  renewenvironment: 3, newtheorem: 2, setlength: 2, setcounter: 2, addtocounter: 2
};

const blank = (s) => s.replace(/[^\n]/g, ' ');

function blankRange(chars, from, to) {
  for (let i = from; i < to; i++) if (chars[i] !== '\n') chars[i] = ' ';
}

/** Replace everything that is not prose with spaces, keeping every offset. */
export function proseMask(text) {
  // Comments first, line by line.
  let work = text
    .split('\n')
    .map((line) => {
      const kept = stripComment(line);
      return kept + blank(line.slice(kept.length));
    })
    .join('\n');

  for (const env of BLANK_ENVS) {
    work = work.replace(new RegExp(`\\\\begin\\{${env}\\*?\\}[\\s\\S]*?\\\\end\\{${env}\\*?\\}`, 'g'), blank);
  }
  work = work
    .replace(/\\\[[\s\S]*?\\\]/g, blank)
    .replace(/\\\([\s\S]*?\\\)/g, blank)
    .replace(/(?<!\\)\$\$[\s\S]*?(?<!\\)\$\$/g, blank)
    .replace(/(?<!\\)\$[^$]*?(?<!\\)\$/g, blank);

  // Accents like \"o or {\'e} or \c{c}: the word they sit in is not ours to judge.
  work = work.replace(/\\["'`^~=.]\s?(?:\{[A-Za-z]\}|[A-Za-z])|\\[cvuHkrbdt]\s*\{[A-Za-z]\}/g, (m) => '\uE000'.repeat(m.length));
  // Escaped specials: \% \& \_ \$ \# \{ \}
  work = work.replace(/\\[%&_$#{}]/g, '  ');

  const chars = [...work];
  const cs = /\\([a-zA-Z@]+)\*?/g;
  for (let m = cs.exec(work); m; m = cs.exec(work)) {
    const start = m.index;
    let end = start + m[0].length;
    if (CODE_ARGS.has(m[1])) {
      let groups = GROUP_COUNT[m[1]] ?? 1;
      for (;;) {
        let p = end;
        while (work[p] === ' ') p++;
        if (work[p] === '[') {
          const close = work.indexOf(']', p);
          if (close === -1) break;
          end = close + 1;
        } else if (work[p] === '{' && groups > 0) {
          const body = balancedGroup(work, p);
          if (body === null) break;
          end = p + body.length + 2;
          groups--;
        } else break;
      }
    }
    blankRange(chars, start, end);
  }
  return chars.join('').replace(/~/g, ' ');
}

const WORD = /[A-Za-zÀ-ɏ\uE000]+(?:['’][A-Za-z]+)*/g;

/**
 * Words worth checking, with 1-based line and column ranges.
 * @returns {{word:string, line:number, startColumn:number, endColumn:number}[]}
 */
export function spellWords(text) {
  const masked = proseMask(text);
  const out = [];
  let lineNo = 1;
  let lineStart = 0;
  const lineStarts = [0];
  for (let i = 0; i < masked.length; i++) if (masked[i] === '\n') lineStarts.push(i + 1);

  let li = 0;
  WORD.lastIndex = 0;
  for (let m = WORD.exec(masked); m; m = WORD.exec(masked)) {
    const word = m[0];
    const before = masked[m.index - 1];
    const after = masked[m.index + word.length];
    if (word.includes('\uE000')) continue;
    if (word.length < 3) continue;
    if (/[0-9_]/.test(before ?? '') || /[0-9_]/.test(after ?? '')) continue;
    if (word === word.toUpperCase()) continue; // acronyms
    if (/[a-z][A-Z]/.test(word)) continue; // camelCase identifiers
    while (li + 1 < lineStarts.length && lineStarts[li + 1] <= m.index) li++;
    lineNo = li + 1;
    lineStart = lineStarts[li];
    out.push({
      word,
      line: lineNo,
      startColumn: m.index - lineStart + 1,
      endColumn: m.index - lineStart + word.length + 1
    });
  }
  return out;
}

/** Misspelled words in `text`, per the checker's `correct(word)`. */
export function misspellings(text, checker) {
  return spellWords(text).filter((w) => !checker.correct(w.word.replace(/’/g, "'")));
}

/**
 * Wraps a Hunspell speller (nspell) with a personal dictionary that outlives
 * the session. The speller itself is loaded on first use; the dictionary is
 * ~0.6 MB, so nothing is fetched unless spell checking is on.
 */
export class SpellChecker {
  #load;
  #speller = null;
  #pending = null;
  #store;
  #personal;
  #listeners = new Set();

  /**
   * @param {() => Promise<{correct(w:string):boolean, suggest(w:string):string[], add(w:string):void}>} load
   * @param {{read(): string[], write(words: string[]): void}} store
   */
  constructor(load, store) {
    this.#load = load;
    this.#store = store;
    this.#personal = new Set(store.read());
  }

  get ready() {
    return this.#speller !== null;
  }

  /** Load the dictionary. Resolves false (and stays unready) if it cannot be loaded. */
  async ensureReady() {
    if (this.#speller) return true;
    this.#pending ??= this.#load()
      .then((s) => {
        for (const w of this.#personal) s.add(w);
        this.#speller = s;
        return true;
      })
      .catch(() => {
        this.#pending = null;
        return false;
      });
    return this.#pending;
  }

  correct(word) {
    return this.#personal.has(word) || (this.#speller?.correct(word) ?? true);
  }

  suggest(word) {
    return this.#speller?.suggest(word).slice(0, 5) ?? [];
  }

  add(word) {
    this.#personal.add(word);
    this.#speller?.add(word);
    this.#store.write([...this.#personal].sort());
    for (const cb of this.#listeners) cb();
  }

  /** Called after a word is added, so open files can be re-checked. */
  onChange(cb) {
    this.#listeners.add(cb);
    return () => this.#listeners.delete(cb);
  }
}
