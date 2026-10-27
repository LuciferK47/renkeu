// A word count for LaTeX source: what a reader would count, not the markup.
// Comments, commands and their option arguments, and maths are skipped; the
// text inside braces is kept. It is an estimate, like every LaTeX word count.

export function countWords(source) {
  const text = source
    .replace(/(^|[^\\])%.*$/gm, '$1') // comments
    .replace(/\\begin\{(equation\*?|align\*?|eqnarray\*?|displaymath|math|verbatim|lstlisting)\}[\s\S]*?\\end\{\1\}/g, ' ')
    .replace(/\$\$[\s\S]*?\$\$/g, ' ')
    .replace(/\\\[[\s\S]*?\\\]/g, ' ')
    .replace(/\$[^$\n]*\$/g, ' ')
    .replace(/\\(?:begin|end|documentclass|usepackage|label|ref|eqref|cref|cite[a-z]*|include|input|bibliography|bibliographystyle|includegraphics)\*?(?:\[[^\]]*\])?(?:\{[^}]*\})*/g, ' ')
    .replace(/\\[a-zA-Z]+\*?/g, ' ') // remaining commands keep their braced text
    .replace(/[{}[\]\\~&^_#]/g, ' ');
  return (text.match(/[\p{L}\p{N}][\p{L}\p{N}'’-]*/gu) ?? []).length;
}
