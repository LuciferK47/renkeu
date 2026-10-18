// Syntax highlighting (Monaco "Monarch" grammars) and editing behaviour for
// LaTeX and BibTeX. Monaco ships neither, hence these.

export const latexConfiguration = {
  comments: { lineComment: '%' },
  brackets: [['{', '}'], ['[', ']'], ['(', ')']],
  autoClosingPairs: [
    { open: '{', close: '}' },
    { open: '[', close: ']' },
    { open: '(', close: ')' },
    { open: '$', close: '$', notIn: ['comment'] },
    { open: '`', close: "'", notIn: ['comment'] }
  ],
  surroundingPairs: [
    { open: '{', close: '}' },
    { open: '[', close: ']' },
    { open: '(', close: ')' },
    { open: '$', close: '$' }
  ],
  // A command, with its backslash and any star, is one word.
  wordPattern: /\\[a-zA-Z@]+\*?|[^\s\\{}[\]()$%&~#^_]+/g
};

export const latexLanguage = {
  defaultToken: '',
  tokenPostfix: '.latex',
  brackets: [
    { open: '{', close: '}', token: 'delimiter.curly' },
    { open: '[', close: ']', token: 'delimiter.square' },
    { open: '(', close: ')', token: 'delimiter.parenthesis' }
  ],
  tokenizer: {
    root: [
      [/%.*$/, 'comment'],
      [/\\(?:begin|end)(?=\s*\{)/, { token: 'keyword', next: '@envname' }],
      [/\\[a-zA-Z@]+\*?/, 'keyword'],
      [/\\[^a-zA-Z@]/, 'keyword.escape'],
      [/\$\$/, { token: 'string.math', next: '@displaymath' }],
      [/\$/, { token: 'string.math', next: '@inlinemath' }],
      [/[{}[\]()]/, '@brackets'],
      [/[&~^_]/, 'operator'],
      [/#\d/, 'variable']
    ],
    envname: [
      [/\s*\{/, 'delimiter.curly'],
      [/[^}]+/, 'type'],
      [/\}/, { token: 'delimiter.curly', next: '@pop' }]
    ],
    inlinemath: [
      [/%.*$/, 'comment'],
      [/\\[a-zA-Z@]+\*?/, 'keyword'],
      [/\\[^a-zA-Z@]/, 'string.math'],
      [/\$/, { token: 'string.math', next: '@pop' }],
      [/[^$\\%]+/, 'string.math']
    ],
    displaymath: [
      [/%.*$/, 'comment'],
      [/\\[a-zA-Z@]+\*?/, 'keyword'],
      [/\\[^a-zA-Z@]/, 'string.math'],
      [/\$\$/, { token: 'string.math', next: '@pop' }],
      [/[^$\\%]+/, 'string.math'],
      [/\$/, 'string.math']
    ]
  }
};

export const bibtexConfiguration = {
  brackets: [['{', '}'], ['(', ')']],
  autoClosingPairs: [
    { open: '{', close: '}' },
    { open: '"', close: '"' },
    { open: '(', close: ')' }
  ],
  surroundingPairs: [{ open: '{', close: '}' }, { open: '"', close: '"' }]
};

export const bibtexLanguage = {
  defaultToken: '',
  tokenPostfix: '.bibtex',
  brackets: [
    { open: '{', close: '}', token: 'delimiter.curly' },
    { open: '(', close: ')', token: 'delimiter.parenthesis' }
  ],
  tokenizer: {
    root: [
      [/@(?:comment)\b.*$/i, 'comment'],
      [/@[A-Za-z]+/, 'keyword'],
      [/[A-Za-z_][\w-]*(?=\s*=)/, 'attribute.name'],
      [/"/, { token: 'string', next: '@quoted' }],
      [/\d+/, 'number'],
      [/[{}()]/, '@brackets'],
      [/[=#,]/, 'operator'],
      [/[A-Za-z_][\w:./-]*/, 'type']
    ],
    quoted: [
      [/[^"\\]+/, 'string'],
      [/\\./, 'string.escape'],
      [/"/, { token: 'string', next: '@pop' }]
    ]
  }
};
