// Monaco, loaded on first use so the app shell paints before the editor's
// ~MBs are parsed, plus the text-model factory the document store uses.

let loading = null;

export function loadMonaco() {
  loading ??= import('monaco-editor/esm/vs/editor/editor.api').then((monaco) => {
    // The id only; highlighting, completion and the outline attach to it in M14.
    monaco.languages.register({ id: 'latex' });
    return monaco;
  });
  return loading;
}

const LANGUAGE = {
  tex: 'latex',
  sty: 'latex',
  cls: 'latex',
  tikz: 'latex',
  bib: 'plaintext'
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
