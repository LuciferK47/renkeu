// Puts a speller's findings on a Monaco model as "Info" squiggles.

import { misspellings } from './spell.js';

const SPELL_OWNER = 'latex-spell';

/** Languages whose text is prose worth checking (BibTeX is mostly names and codes). */
export const isProseFile = (path) => /\.(tex|sty|cls)$/i.test(path);

export function applySpellMarkers(monaco, model, checker) {
  const found = checker ? misspellings(model.getValue(), checker) : [];
  monaco.editor.setModelMarkers(
    model,
    SPELL_OWNER,
    found.map((w) => ({
      severity: monaco.MarkerSeverity.Info,
      message: `Unknown word: ${w.word}`,
      startLineNumber: w.line,
      endLineNumber: w.line,
      startColumn: w.startColumn,
      endColumn: w.endColumn,
      source: 'spell'
    }))
  );
  return found.length;
}

export function clearSpellMarkers(monaco, model) {
  monaco.editor.setModelMarkers(model, SPELL_OWNER, []);
}
