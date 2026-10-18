// Monaco, loaded on first use so the app shell paints before the editor's
// ~MBs are parsed, plus the text-model factory the document store uses.

import { registerLatex } from './latex/register.js';

let loading = null;
/** Where completion gets project facts (labels, citations, files); set by the app. */
let projectIndex = null;

export function useProjectIndex(index) {
  projectIndex = index;
}

export function loadMonaco() {
  // editor.api alone is the bare core: no suggest widget, snippets, comment
  // toggling, find or folding. editor.all registers those contributions.
  loading ??= Promise.all([
    import('monaco-editor/esm/vs/editor/editor.all.js'),
    import('monaco-editor/esm/vs/editor/editor.api')
  ]).then(([, monaco]) => {
    registerLatex(monaco, () => projectIndex);
    return monaco;
  });
  return loading;
}

const LANGUAGE = {
  tex: 'latex',
  sty: 'latex',
  cls: 'latex',
  tikz: 'latex',
  bib: 'bibtex'
};

export function languageFor(path) {
  const ext = path.split('.').pop()?.toLowerCase() ?? '';
  return LANGUAGE[ext] ?? 'plaintext';
}

/** Builds one Monaco model per open file. */
export const monacoModels = {
  async create(path, text) {
    const monaco = await loadMonaco();
    return monaco.editor.createModel(text, languageFor(path), monaco.Uri.file(`/${path}`));
  },
  dispose(model) {
    model.dispose();
  }
};
