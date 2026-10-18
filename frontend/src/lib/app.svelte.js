// The application's state, built once. Components receive these as props (or
// import them) instead of reaching into a global store each.

import { CommandRegistry } from './commands.js';
import { CompileState } from './state/compile.svelte.js';
import { DocumentStore } from './state/documents.svelte.js';
import { ProjectState } from './state/project.svelte.js';
import { UiState } from './state/ui.svelte.js';
import { AiState } from './state/ai.svelte.js';
import { PaperState } from './state/paper.svelte.js';
import { monacoModels, useEditorServices } from './monaco.js';
import { readPref, writePref } from './storage.js';
import { SpellChecker } from './latex/spell.js';
import { ProjectIndex } from './latex/index.js';

export const documents = new DocumentStore(monacoModels);
export const compile = new CompileState();
export const project = new ProjectState();
export const ui = new UiState();
export const commands = new CommandRegistry();
export const ai = new AiState({ documents, compile, project, ui });
export const paper = new PaperState();

export const latexIndex = new ProjectIndex({ project, documents });

// The dictionary is fetched and parsed only when spell checking first runs.
export const spell = new SpellChecker(
  async () => (await import('./latex/spellLoader.js')).loadEnglish(),
  { read: () => readPref('latex-ai:dictionary', []), write: (words) => writePref('latex-ai:dictionary', words) }
);

useEditorServices({ index: latexIndex, spell });
