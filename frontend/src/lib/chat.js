import { createChatController } from '../controllers/ChatController.js';
import { compile, documents, project, ui } from './app.svelte.js';

/** The AI conversation, wired to the app's state. */
export const chat = createChatController({ documents, compile, project, ui });
