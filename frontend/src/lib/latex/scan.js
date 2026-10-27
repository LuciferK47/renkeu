// One pass over a LaTeX source that finds what editing support needs: the
// outline, labels, files it pulls in, bibliography resources, and macros it
// defines. Line-based and comment-aware; it is not a TeX parser, and says so
// by only recognising constructs whose syntax is unambiguous.

const SECTION_LEVELS = { part: 0, chapter: 1, section: 2, subsection: 3, subsubsection: 4, paragraph: 5, subparagraph: 6 };

/** The line without its comment (a `%` not preceded by a backslash run of odd length). */
export function stripComment(line) {
  for (let i = 0; i < line.length; i++) {
    if (line[i] !== '%') continue;
    let slashes = 0;
    for (let j = i - 1; j >= 0 && line[j] === '\\'; j--) slashes++;
    if (slashes % 2 === 0) return line.slice(0, i);
  }
  return line;
}

/** Contents of the `{…}` group opening at `text[start]`, balanced; null if unclosed. */
export function balancedGroup(text, start) {
  if (text[start] !== '{') return null;
  let depth = 0;
  for (let i = start; i < text.length; i++) {
    const c = text[i];
    if (c === '\\') i++;
    else if (c === '{') depth++;
    else if (c === '}' && --depth === 0) return text.slice(start + 1, i);
  }
  return null;
}

const VERBATIM_OPEN = /\\begin\{(verbatim\*?|lstlisting|minted|comment)\}/;
const VERBATIM_CLOSE = /\\end\{(verbatim\*?|lstlisting|minted|comment)\}/;

const SECTION = /\\(part|chapter|section|subsection|subsubsection|paragraph|subparagraph)(\*?)\s*(?:\[[^\]]*\])?\s*(?=\{)/g;
const LABEL = /\\label\s*\{([^{}]+)\}/g;
const INCLUDE = /\\(input|include|subfile)\s*\{([^{}]+)\}/g;
const GRAPHICS = /\\includegraphics\*?\s*(?:\[[^\]]*\])?\s*\{([^{}]+)\}/g;
const BIB_LEGACY = /\\bibliography\s*\{([^{}]+)\}/g;
const BIB_RESOURCE = /\\addbibresource\s*(?:\[[^\]]*\])?\s*\{([^{}]+)\}/g;
const PACKAGE = /\\(?:usepackage|RequirePackage)\s*(?:\[[^\]]*\])?\s*\{([^{}]+)\}/g;
const MACRO = /\\(?:newcommand|renewcommand|providecommand|DeclareMathOperator)\*?\s*\{?\s*\\([a-zA-Z@]+)/g;
const DEF = /\\(?:def|let)\s*\\([a-zA-Z@]+)/g;
const NEWENV = /\\(?:newenvironment|renewenvironment)\*?\s*\{([^{}]+)\}/g;
const ROOT_MAGIC = /^\s*%\s*!\s*TEX\s+root\s*=\s*(.+?)\s*$/i;

function each(re, line, fn) {
  re.lastIndex = 0;
  for (let m = re.exec(line); m; m = re.exec(line)) fn(m);
}

const commaList = (s) => s.split(',').map((x) => x.trim()).filter(Boolean);

/**
 * @returns {{
 *   sections: {level:number, title:string, line:number, starred:boolean, command:string}[],
 *   labels: {name:string, line:number, text:string}[],
 *   includes: {kind:'input'|'include'|'subfile'|'graphics'|'bib', target:string, line:number}[],
 *   packages: string[],
 *   macros: string[],
 *   macroDefs: {name:string, line:number, text:string}[],
 *   environments: string[],
 *   root: string|null
 * }}
 */
export function scan(text) {
  const out = { sections: [], labels: [], includes: [], packages: [], macros: [], macroDefs: [], environments: [], root: null };
  const lines = text.split(/\r\n|\r|\n/);
  let verbatim = false;

  lines.forEach((raw, index) => {
    const lineNo = index + 1;
    if (lineNo <= 20) {
      const magic = ROOT_MAGIC.exec(raw);
      if (magic) out.root = magic[1];
    }
    const line = stripComment(raw);

    if (verbatim) {
      if (VERBATIM_CLOSE.test(line)) verbatim = false;
      return;
    }
    if (VERBATIM_OPEN.test(line) && !VERBATIM_CLOSE.test(line)) {
      verbatim = true;
    }

    each(SECTION, line, (m) => {
      const title = balancedGroup(line, m.index + m[0].length);
      if (title === null) return;
      out.sections.push({
        level: SECTION_LEVELS[m[1]],
        title: title.replace(/\\(?:label|footnote)\{[^}]*\}/g, '').replace(/\s+/g, ' ').trim(),
        line: lineNo,
        starred: m[2] === '*',
        command: m[1]
      });
    });
    each(LABEL, line, (m) => out.labels.push({ name: m[1].trim(), line: lineNo, text: line.trim() }));
    each(INCLUDE, line, (m) => out.includes.push({ kind: m[1], target: m[2].trim(), line: lineNo }));
    each(GRAPHICS, line, (m) => out.includes.push({ kind: 'graphics', target: m[1].trim(), line: lineNo }));
    each(BIB_LEGACY, line, (m) => commaList(m[1]).forEach((t) => out.includes.push({ kind: 'bib', target: t.endsWith('.bib') ? t : `${t}.bib`, line: lineNo })));
    each(BIB_RESOURCE, line, (m) => out.includes.push({ kind: 'bib', target: m[1].trim(), line: lineNo }));
    each(PACKAGE, line, (m) => out.packages.push(...commaList(m[1])));
    const defined = (m) => {
      out.macros.push(m[1]);
      out.macroDefs.push({ name: m[1], line: lineNo, text: line.trim() });
    };
    each(MACRO, line, defined);
    each(DEF, line, defined);
    each(NEWENV, line, (m) => out.environments.push(m[1].trim()));
  });
  return out;
}
