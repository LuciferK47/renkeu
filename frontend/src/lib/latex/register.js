// Teaches Monaco LaTeX and BibTeX: grammars, editing behaviour, completion.

import { latexConfiguration, latexLanguage, bibtexConfiguration, bibtexLanguage } from './monarch.js';
import { contextAt, suggest, suggestBib } from './completion.js';
import { referenceAt, definitionFor, hoverFor } from './navigate.js';
import { packageFixFor } from './insert.js';

/**
 * @param monaco the Monaco API
 * @param {() => ({ index?: { snapshot(): object, ensureLoaded(): Promise<void> },
 *   spell?: { suggest(w: string): string[], add(w: string): void },
 *   openInsert?: (tab: string) => void } | null)} services
 *   what the editor can draw on; a function so the app can supply it late
 */
export function registerLatex(monaco, services) {
  const { languages } = monaco;
  const index = () => services()?.index ?? null;

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

  // Hover: say what a reference points at, and flag the ones that point at nothing.
  languages.registerHoverProvider('latex', {
    async provideHover(model, position) {
      const idx = index();
      if (!idx) return null;
      const ref = referenceAt(model.getLineContent(position.lineNumber), position.column);
      if (!ref) return null;
      await idx.ensureLoaded();
      const snap = idx.snapshot();
      const text = hoverFor(ref, snap);
      if (!text) return null;
      const followable = definitionFor(ref, snap) ? '\n\n*Ctrl+click or F12 to open*' : '';
      return {
        range: {
          startLineNumber: position.lineNumber,
          endLineNumber: position.lineNumber,
          startColumn: ref.start + 1,
          endColumn: ref.end + 1
        },
        contents: [{ value: text + followable }]
      };
    }
  });

  // Spelling quick fixes: the markers come from the editor pane, the fixes from here.
  monaco.editor.registerCommand('latex-ai.spell.add', (_accessor, word) => services()?.spell?.add(word));
  monaco.editor.registerCommand('latex-ai.insert', (_accessor, tab) => services()?.openInsert?.(tab));
  languages.registerCodeActionProvider('latex', {
    provideCodeActions(model, _range, context) {
      const spell = services()?.spell;
      const actions = [];
      for (const marker of context.markers) {
        // Compile errors we can fix without asking an AI.
        if (marker.source === 'LaTeX') {
          // Only read the document for the one error this can fix.
          const fix = /^Undefined control sequence/.test(marker.message ?? '')
            ? packageFixFor({
                message: marker.message,
                line: model.getLineContent(marker.startLineNumber),
                text: model.getValue()
              })
            : null;
          if (fix) {
            const at = model.getPositionAt(fix.edit.offset);
            actions.push({
              title: `Add \\usepackage{${fix.package}} (defines \\${fix.command})`,
              kind: 'quickfix',
              diagnostics: [marker],
              isPreferred: true,
              edit: {
                edits: [
                  {
                    resource: model.uri,
                    versionId: model.getVersionId(),
                    textEdit: {
                      range: { startLineNumber: at.lineNumber, startColumn: at.column, endLineNumber: at.lineNumber, endColumn: at.column },
                      text: fix.edit.text
                    }
                  }
                ]
              }
            });
          } else if (/Citation .* undefined/i.test(marker.message)) {
            actions.push({
              title: 'Insert a citation by search or DOI…',
              kind: 'quickfix',
              diagnostics: [marker],
              command: { id: 'latex-ai.insert', title: 'Insert citation', arguments: ['citation'] }
            });
          }
          continue;
        }
        if (marker.source !== 'spell' || !spell) continue;
        const range = {
          startLineNumber: marker.startLineNumber,
          endLineNumber: marker.endLineNumber,
          startColumn: marker.startColumn,
          endColumn: marker.endColumn
        };
        const word = model.getValueInRange(range);
        spell.suggest(word).forEach((fix, i) =>
          actions.push({
            title: `Change to “${fix}”`,
            kind: 'quickfix',
            diagnostics: [marker],
            isPreferred: i === 0,
            edit: { edits: [{ resource: model.uri, versionId: model.getVersionId(), textEdit: { range, text: fix } }] }
          })
        );
        actions.push({
          title: `Add “${word}” to the dictionary`,
          kind: 'quickfix',
          diagnostics: [marker],
          command: { id: 'latex-ai.spell.add', title: 'Add to dictionary', arguments: [word] }
        });
      }
      return { actions, dispose() {} };
    }
  });
}
