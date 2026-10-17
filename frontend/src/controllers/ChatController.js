import * as aiService from '../services/aiService.js';
import * as fileService from '../services/fileService.js';
import { aiStore, addMessage, setProposal, clearProposal } from '../stores/aiStore.js';
import { uiStore, pushToast } from '../stores/uiStore.js';
import { fileStore } from '../stores/fileStore.js';
import { compileDocument } from './CompileController.js';

function getProposalId() {
  let id = null;
  aiStore.subscribe((a) => (id = a.currentProposal?.proposal_id ?? null))();
  return id;
}

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
  } catch (e) {
    // The onError callback above only covers failures *during* the stream. If
    // the call itself rejects -- the request never connects, or the server
    // answers 500 -- nothing had handled it: App.svelte invokes this from an
    // event handler with no catch, so the rejection went unhandled and the user
    // saw the status return to "Ready" with no message at all.
    append(`Error: ${e.message}`);
    addMessage('assistant', transcript.join('\n'));
    pushToast(`AI request failed: ${e.message}`, 'error');
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

  try {
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
  } catch (e) {
    // Same reason as handleChatMessage: a rejected call is not a stream error,
    // and the command palette does not catch either.
    uiStore.update((ui) => ({ ...ui, status: `AI error: ${e.message}` }));
    pushToast(`AI request failed: ${e.message}`, 'error');
    return null;
  }

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
  const proposalId = getProposalId();
  try {
    let res;
    try {
      res = await aiService.applySelectedHunks(filePath, edits, selectedHunkIds, expectedEtag, {
        proposalId
      });
    } catch (err) {
      // The file moved while the user was reviewing. If the server still has
      // the proposal, ask it to move the hunks onto the new text; it does so
      // only when every hunk still fits, else it answers 409 again.
      if (err.status !== 409 || !proposalId) throw err;
      res = await aiService.applySelectedHunks(filePath, edits, selectedHunkIds, expectedEtag, {
        proposalId,
        rebase: true
      });
      pushToast('The file changed while you reviewed; your changes were moved onto it', 'info');
    }
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
