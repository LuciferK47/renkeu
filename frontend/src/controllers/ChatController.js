import * as aiService from '../services/aiService.js';
import { aiStore, addMessage, setProposal, clearProposal } from '../stores/aiStore.js';

/**
 * The AI conversation: drives the agent stream, shows its events, and applies
 * the hunks the user accepts. Built around the app's state so it can be tested
 * with stand-ins; `chat` below is the instance the app uses.
 *
 * @param {{ documents, compile, project, ui }} ctx
 */
export function createChatController({ documents, compile, project, ui }) {
  const activePath = () => documents.active?.path ?? null;

  /**
   * Drive the agent loop for a chat message.
   * Consumes the locked SSE taxonomy ({ event, data }) — NOT a `data.type`
   * field, which never existed on the wire.
   */
  async function handleChatMessage(content) {
    const filePath = activePath();
    if (!filePath) {
      ui.toast('Open a file first', 'error');
      return;
    }

    addMessage('user', content);
    ui.setStatus('AI is thinking...');
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
            case 'step':
              append(`Attempt ${data.iteration}: ${data.edit_count} edit(s)`);
              break;
            case 'proposal':
              append(`Proposed ${data.edit_count} edit(s).`);
              ui.show('chat');
              setProposal({ ...data, file: filePath });
              break;
            case 'applying':
              append('Applying edits to a working copy…');
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
          ui.toast(`AI stream error: ${error.message}`, 'error');
        }
      );

      addMessage('assistant', transcript.join('\n') || 'No response from agent.');
    } catch (e) {
      // The onError callback above only covers failures *during* the stream. If
      // the call itself rejects -- the request never connects, or the server
      // answers 500 -- nothing had handled it, and the user saw the status
      // return to "Ready" with no message at all.
      append(`Error: ${e.message}`);
      addMessage('assistant', transcript.join('\n'));
      ui.toast(`AI request failed: ${e.message}`, 'error');
    } finally {
      aiStore.update((a) => ({
        ...a,
        status: a.currentProposal ? 'proposal' : 'idle',
        streaming: false,
        streamingMessage: ''
      }));
      ui.setStatus('Ready');
    }
  }

  /** One-shot plan (no compile loop), used by the command palette. */
  async function requestAIPlan(instruction) {
    const filePath = activePath();
    if (!filePath) {
      ui.toast('Open a file first', 'error');
      return null;
    }

    ui.setStatus('Planning with AI...');
    let proposal = null;

    try {
      await aiService.streamAIPlan(
        filePath,
        instruction,
        ({ event, data }) => {
          if (event === 'proposal') {
            proposal = data;
            setProposal({ ...data, file: filePath });
            ui.show('chat');
            ui.setStatus('AI plan ready');
          } else if (event === 'error') {
            ui.setStatus(`AI error: ${data.message}`);
            ui.toast(data.message, 'error');
          }
        },
        (error) => {
          ui.setStatus(`AI error: ${error.message}`);
          ui.toast(error.message, 'error');
        }
      );
    } catch (e) {
      // A rejected call is not a stream error, and the palette does not catch.
      ui.setStatus(`AI error: ${e.message}`);
      ui.toast(`AI request failed: ${e.message}`, 'error');
      return null;
    }

    return proposal;
  }

  let proposalOf = () => {
    let current = null;
    aiStore.subscribe((a) => (current = a.currentProposal))();
    return current;
  };

  /**
   * Apply the user-selected hunks of a proposal.
   * `selectedHunkIds` are server-computed ids. The write is conditional on the
   * file still being what the proposal was made against (`etag` of the
   * proposal, else the preview's), so a file that moved during review is
   * refused — or, if the server still holds the proposal, rebased.
   */
  async function handleApplyHunks({ edits, selectedHunkIds, expectedEtag } = {}) {
    const filePath = activePath();
    if (!edits?.length) {
      ui.toast('No proposal to apply', 'error');
      return false;
    }
    if (!selectedHunkIds?.length) {
      ui.toast('No hunks selected', 'info');
      return false;
    }

    const proposal = proposalOf();
    const proposalId = proposal?.proposal_id ?? null;
    const base = proposal?.etag ?? expectedEtag;

    ui.setStatus('Applying selected edits...');
    try {
      let res;
      try {
        res = await aiService.applySelectedHunks(filePath, edits, selectedHunkIds, base, {
          proposalId
        });
      } catch (err) {
        // The file moved while the user was reviewing. If the server still has
        // the proposal, ask it to move the hunks onto the new text; it does so
        // only when every hunk still fits, else it answers 409 again.
        if (err.status !== 409 || !proposalId) throw err;
        res = await aiService.applySelectedHunks(filePath, edits, selectedHunkIds, base, {
          proposalId,
          rebase: true
        });
        ui.toast('The file changed while you reviewed; your changes were moved onto it', 'info');
      }
      if (!res?.success) throw new Error(res?.error || 'Unknown error');

      // The server wrote the files; re-read every one that is open so the
      // editor and the disk cannot drift apart.
      const written = new Set([filePath, ...(res.files ?? []).map((f) => f.path)]);
      for (const path of written) {
        if (documents.get(path)) await documents.reload(path);
      }
      await project.refresh();

      clearProposal();
      ui.setStatus('Selected edits applied');
      ui.toast('Selected changes applied', 'success');
      compile.start(filePath, { main: project.mainOverride });
      return true;
    } catch (err) {
      // 409: the file moved under the review. Say so specifically — a generic
      // failure would invite the user to just retry into the same conflict.
      if (err.status === 409) {
        ui.setStatus('File changed during review');
        ui.toast('File changed while you were reviewing — reload and re-plan', 'error');
      } else {
        ui.setStatus(`Apply failed: ${err.message}`);
        ui.toast(`Apply failed: ${err.message}`, 'error');
      }
      return false;
    }
  }

  return { handleChatMessage, requestAIPlan, handleApplyHunks };
}
