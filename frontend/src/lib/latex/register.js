// Teaches Monaco LaTeX and BibTeX: grammars, editing behaviour, completion.

import { latexConfiguration, latexLanguage, bibtexConfiguration, bibtexLanguage } from './monarch.js';
import { contextAt, suggest, suggestBib } from './completion.js';

/**
 * @param monaco the Monaco API
 * @param {{ snapshot(): object, ensureLoaded(): Promise<void> } | (() => object | null)} source
 *   where project facts come from; a function so it can be supplied late
 */
export function registerLatex(monaco, source) {
  const { languages } = monaco;
  const index = () => (typeof source === 'function' ? source() : source);

  languages.register({ id: 'latex', extensions: ['.tex', '.sty', '.cls', '.tikz'], aliases: ['LaTeX'] });
  languages.setLanguageConfiguration('latex', latexConfiguration);
  languages.setMonarchTokensProvider('latex', latexLanguage);

  languages.register({ id: 'bibtex', extensions: ['.bib'], aliases: ['BibTeX'] });
  languages.setLanguageConfiguration('bibtex', bibtexConfiguration);
  languages.setMonarchTokensProvider('bibtex', bibtexLanguage);

  const Kind = languages.CompletionItemKind;
  const kindFor = (ctx, item) =>
    ({
      cite: Kind.Reference,
      ref: Kind.Reference,
      include: Kind.File,
      graphics: Kind.File,
      bibfile: Kind.File,
      package: Kind.Module,
      'env-begin': Kind.Struct,
      'env-end': Kind.Struct
    })[ctx.kind] ?? (item.snippet ? Kind.Snippet : Kind.Function);

  const toItem = (item, ctx, range) => ({
    label: item.label,
    kind: kindFor(ctx, item),
    detail: item.detail,
    documentation: item.documentation,
    insertText: item.insertText,
    insertTextRules: item.snippet ? languages.CompletionItemInsertTextRule.InsertAsSnippet : undefined,
    filterText: item.filterText ?? item.label,
    // Keep project symbols above the built-in list.
    sortText: ctx.kind === 'command' && item.detail !== 'defined in this project' ? `1${item.label}` : `0${item.label}`,
    range
  });

  languages.registerCompletionItemProvider('latex', {
    triggerCharacters: ['\\', '{', ','],
    async provideCompletionItems(model, position) {
      const line = model.getLineContent(position.lineNumber);
      // A cursor in a comment completes nothing.
      const before = line.slice(0, position.column - 1);
      if (/(?:^|[^\\])(?:\\\\)*%/.test(before)) return { suggestions: [] };
      const ctx = contextAt(line, position.column - 1);
      if (!ctx) return { suggestions: [] };
      const idx = index();
      if (!idx) return { suggestions: [] };
      await idx.ensureLoaded();
      const range = {
        startLineNumber: position.lineNumber,
        endLineNumber: position.lineNumber,
        startColumn: ctx.start + 1,
        endColumn: ctx.end + 1
      };
      return { suggestions: suggest(ctx, idx.snapshot()).map((item) => toItem(item, ctx, range)) };
    }
  });

  languages.registerCompletionItemProvider('bibtex', {
    triggerCharacters: ['@'],
    provideCompletionItems(model, position) {
      const line = model.getLineContent(position.lineNumber).slice(0, position.column - 1);
      const { start, items } = suggestBib(line);
      const range = {
        startLineNumber: position.lineNumber,
        endLineNumber: position.lineNumber,
        startColumn: start + 1,
        endColumn: position.column
      };
      return {
        suggestions: items.map((i) => ({
          label: i.label,
          kind: Kind.Snippet,
          detail: i.detail,
          insertText: i.insertText,
          insertTextRules: languages.CompletionItemInsertTextRule.InsertAsSnippet,
          range
        }))
      };
    }
  });
}
