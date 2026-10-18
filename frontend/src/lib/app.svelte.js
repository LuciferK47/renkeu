// The application's state, built once. Components receive these as props (or
// import them) instead of reaching into a global store each.

import { CommandRegistry } from './commands.js';
import { CompileState } from './state/compile.svelte.js';
import { DocumentStore } from './state/documents.svelte.js';
import { ProjectState } from './state/project.svelte.js';
import { UiState } from './state/ui.svelte.js';
import { monacoModels, useProjectIndex } from './monaco.js';
import { ProjectIndex } from './latex/index.js';

export const documents = new DocumentStore(monacoModels);
export const compile = new CompileState();
export const project = new ProjectState();
export const ui = new UiState();
export const commands = new CommandRegistry();

export const latexIndex = new ProjectIndex({ project, documents });
useProjectIndex(latexIndex);
