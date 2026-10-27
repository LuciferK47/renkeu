// Editor colour themes for LaTeX and BibTeX. Monaco's stock themes colour a
// handful of generic tokens; the grammars in latex/monarch.js emit finer ones
// (commands, escapes, maths, environment names, BibTeX fields), and these give
// each a colour of its own. Commands are the warm hue because they are what
// you read first; maths is cool so it stands apart from prose.

const DARK_THEME = 'latexai-dark';
const LIGHT_THEME = 'latexai-light';

export const editorThemeName = (dark) => (dark ? DARK_THEME : LIGHT_THEME);

const dark = {
  base: 'vs-dark',
  inherit: true,
  rules: [
    { token: 'keyword', foreground: 'ff9a5c' },
    { token: 'keyword.escape', foreground: '8a96a6' },
    { token: 'string.math', foreground: '8fd3ff' },
    { token: 'comment', foreground: '6c7a89', fontStyle: 'italic' },
    { token: 'delimiter.curly', foreground: '9aa6b6' },
    { token: 'delimiter.square', foreground: '9aa6b6' },
    { token: 'delimiter.parenthesis', foreground: '9aa6b6' },
    { token: 'attribute.name', foreground: 'ffc247' },
    { token: 'number', foreground: 'b5e08a' },
    { token: 'operator', foreground: 'ff7a90' },
    { token: 'string', foreground: 'a6e3a1' },
    { token: 'string.escape', foreground: '8a96a6' },
    { token: 'type', foreground: 'ffd08a' },
    { token: 'variable', foreground: 'ff9aa8' }
  ],
  colors: {
    'editor.background': '#0b0e13',
    'editor.foreground': '#e6e2da',
    'editor.lineHighlightBackground': '#121820',
    'editor.selectionBackground': '#e2512f44',
    'editor.inactiveSelectionBackground': '#e2512f22',
    'editorCursor.foreground': '#ff8a3d',
    'editorLineNumber.foreground': '#4b5767',
    'editorLineNumber.activeForeground': '#b2bac0',
    'editorIndentGuide.background1': '#1b232d',
    'editorWidget.background': '#10141a',
    'editorWidget.border': '#2f3a49',
    'editorSuggestWidget.background': '#10141a',
    'editorSuggestWidget.border': '#2f3a49',
    'editorSuggestWidget.selectedBackground': '#1e2530'
  }
};

const light = {
  base: 'vs',
  inherit: true,
  rules: [
    { token: 'keyword', foreground: 'b8381f' },
    { token: 'keyword.escape', foreground: '8a8f95' },
    { token: 'string.math', foreground: '1f6f9a' },
    { token: 'comment', foreground: '7b8794', fontStyle: 'italic' },
    { token: 'delimiter.curly', foreground: '7a8089' },
    { token: 'delimiter.square', foreground: '7a8089' },
    { token: 'delimiter.parenthesis', foreground: '7a8089' },
    { token: 'attribute.name', foreground: '9a6827' },
    { token: 'number', foreground: '2f7d4d' },
    { token: 'operator', foreground: 'b02a45' },
    { token: 'string', foreground: '2f7d4d' },
    { token: 'string.escape', foreground: '8a8f95' },
    { token: 'type', foreground: '8a5a14' },
    { token: 'variable', foreground: 'b02a45' }
  ],
  colors: {
    'editor.background': '#fbf8f2',
    'editor.foreground': '#202221',
    'editor.lineHighlightBackground': '#f3eee3',
    'editor.selectionBackground': '#e2512f33',
    'editorCursor.foreground': '#c23a1c',
    'editorLineNumber.foreground': '#a8a499',
    'editorLineNumber.activeForeground': '#5d5b55'
  }
};

/** Register both themes. Safe to call more than once. */
export function defineEditorThemes(monaco) {
  monaco.editor.defineTheme(DARK_THEME, dark);
  monaco.editor.defineTheme(LIGHT_THEME, light);
}
