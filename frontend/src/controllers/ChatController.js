import * as aiService from '../services/aiService.js';
import * as fileService from '../services/fileService.js';
import { aiStore, addMessage, setProposal, clearProposal } from '../stores/aiStore.js';
import { uiStore, pushToast } from '../stores/uiStore.js';
import { fileStore } from '../stores/fileStore.js';
import { compileDocument } from './CompileController.js';

function getFileStoreValue() {
  let value;
  const unsubscribe = fileStore.subscribe((v) => (value = v));
  unsubscribe();
  return value;
}

/**
 * Drive the agent loop for a chat message.
 * Consumes the locked SSE taxonomy ({ event, data }) — NOT a `data.type`
 * field, which never existed on the wire.
 */
export async function handleChatMessage(content) {
  const { filePath } = getFileStoreValue();
  if (!filePath) {
    pushToast('Open a file first', 'error');
    return;
  }

  addMessage('user', content);
  uiStore.update((ui) => ({ ...ui, status: 'AI is thinking...' }));
  aiStore.update((a) => ({ ...a, status: 'streaming', streamingMessage: '', error: null }));

  const transcript = [];
  const append = (line) => {
    transcript.push(line);
    aiStore.update((a) => ({ ...a, streamingMessage: transcript.join('\n') }));
  };

  try {
    await aiService.agentLoopStream(
      filePath,
      content,
      ({ event, data }) => {
        switch (event) {
          case 'thinking':
            append(`Iteration ${data.iteration}: ${data.message ?? 'thinking…'}`);
            break;
          case 'progress':
            append(data.message ?? 'working…');
            break;
          case 'proposal':
            append(`Proposed ${data.edit_count} edit(s).`);
            uiStore.update((ui) => ({ ...ui, activeRightTab: 'ai' }));
            setProposal(data);
            break;
          case 'applying':
            append('Applying edits to scratchpad…');
            break;
          case 'compiled':
            append(
              data.success
                ? 'Compiled successfully.'
                : `Compile failed: ${data.error_summary ?? 'unknown error'}`
            );
            break;
          case 'awaiting_approval':
            append('Waiting for your review.');
            aiStore.update((a) => ({ ...a, status: 'proposal' }));
            break;
          case 'done':
            append(data.success ? 'Done.' : `Stopped: ${data.message ?? 'no success'}`);
            break;
          case 'error':
            append(`Error: ${data.message}`);
            break;
          default:
            break;
        }
      },
      (error) => {
        append(`Error: ${error.message}`);
        pushToast(`AI stream error: ${error.message}`, 'error');
      }
    );

    addMessage('assistant', transcript.join('\n') || 'No response from agent.');
  } finally {
    aiStore.update((a) => ({
      ...a,
      status: a.currentProposal ? 'proposal' : 'idle',
      streaming: false,
      streamingMessage: ''
    }));
    uiStore.update((ui) => ({ ...ui, status: 'Ready' }));
  }
}

/** One-shot plan (no compile loop), used by the command palette. */
export async function requestAIPlan(instruction) {
  const { filePath } = getFileStoreValue();
  if (!filePath) {
    pushToast('Open a file first', 'error');
    return null;
  }

  uiStore.update((ui) => ({ ...ui, status: 'Planning with AI...' }));
  let proposal = null;

  await aiService.streamAIPlan(
    filePath,
    instruction,
    ({ event, data }) => {
      if (event === 'proposal') {
        proposal = data;
        setProposal(data);
        uiStore.update((ui) => ({ ...ui, status: 'AI plan ready', activeRightTab: 'ai' }));
      } else if (event === 'error') {
        uiStore.update((ui) => ({ ...ui, status: `AI error: ${data.message}` }));
        pushToast(data.message, 'error');
      }
    },
    (error) => {
      uiStore.update((ui) => ({ ...ui, status: `AI error: ${error.message}` }));
      pushToast(error.message, 'error');
    }
  );

  return proposal;
}

/**
 * Apply the user-selected hunks of a proposal.
 * `selectedHunkIds` are server-computed ids; `expectedEtag` (from the preview)
 * makes the write conditional on the file not having changed during review.
 */
export async function handleApplyHunks({ edits, selectedHunkIds, expectedEtag } = {}) {
  const { filePath } = getFileStoreValue();
  if (!edits?.length) {
    pushToast('No proposal to apply', 'error');
    return false;
  }
  if (!selectedHunkIds?.length) {
    pushToast('No hunks selected', 'info');
    return false;
  }

  uiStore.update((ui) => ({ ...ui, status: 'Applying selected edits...' }));
  try {
    const res = await aiService.applySelectedHunks(
      filePath,
      edits,
      selectedHunkIds,
      expectedEtag
    );
    if (!res?.success) throw new Error(res?.error || 'Unknown error');

    const reloaded = await fileService.loadFile(filePath);
    fileStore.update((f) => ({
      ...f,
      content: reloaded.content,
      savedContent: reloaded.content,
      etag: reloaded.eTag,
      dirty: false
    }));

    clearProposal();
    uiStore.update((ui) => ({ ...ui, status: 'Selected edits applied', activeRightTab: 'pdf' }));
    pushToast('Selected changes applied', 'success');
    compileDocument(filePath);
    return true;
  } catch (err) {
    // 409: the file moved under the review. Say so specifically — a generic
    // failure would invite the user to just retry into the same conflict.
    if (err.status === 409) {
      uiStore.update((ui) => ({ ...ui, status: 'File changed during review' }));
      pushToast('File changed while you were reviewing — reload and re-plan', 'error');
    } else {
      uiStore.update((ui) => ({ ...ui, status: `Apply failed: ${err.message}` }));
      pushToast(`Apply failed: ${err.message}`, 'error');
    }
    return false;
  }
}
