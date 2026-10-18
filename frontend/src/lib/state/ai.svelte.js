// The AI conversation: runs the agent (or a one-shot plan), keeps a structured
// timeline of what it did, holds the proposal awaiting review, and applies the
// hunks the user accepts.
//
// Replaces the svelte/store `aiStore` and the controller that fed it. A run can
// be stopped: aborting the request closes the connection, and the server
// cancels the planner call and any compile bound to it.

import * as defaultService from '../../services/aiService.js';

let nextId = 1;
const id = () => nextId++;

/** What a run's `outcome` can be, for the UI. */
export const OUTCOME = {
  success: 'success',
  failed: 'failed',
  cancelled: 'cancelled',
  review: 'review'
};

export class AiState {
  /** { id, role: 'user' | 'assistant', text?, run? } */
  messages = $state([]);
  /** The proposal awaiting review, or null. */
  proposal = $state(null);
  /** idle | running | review | applying */
  status = $state('idle');

  #documents;
  #compile;
  #project;
  #ui;
  #service;
  #abort = null;

  constructor({ documents, compile, project, ui, service = defaultService }) {
    this.#documents = documents;
    this.#compile = compile;
    this.#project = project;
    this.#ui = ui;
    this.#service = service;
  }

  get running() {
    return this.status === 'running';
  }

  /** Start over: forget the conversation and any proposal. Stops a run first. */
  newConversation() {
    this.cancel();
    this.messages = [];
    this.proposal = null;
    this.status = 'idle';
  }

  /** Run the self-healing agent for a chat message. */
  async send(instruction) {
    return this.#run(instruction, (path, onEvent, onError, signal) =>
      this.#service.agentLoopStream(path, instruction, onEvent, onError, { signal })
    );
  }

  /**
   * One-shot plan without the compile loop (the "Ask the AI" box). Returns the
   * proposal, or null if there was none.
   */
  async plan(instruction) {
    await this.#run(instruction, (path, onEvent, onError, signal) =>
      this.#service.streamAIPlan(path, instruction, onEvent, onError, { signal })
    );
    return this.proposal;
  }

  /** Stop the current run. Safe to call when nothing is running. */
  cancel() {
    this.#abort?.abort();
  }

  /** Put an externally made proposal (a visual fix) up for review. */
  showProposal(proposal) {
    this.proposal = proposal;
    if (this.status === 'idle') this.status = 'review';
    this.#ui.show('chat');
  }

  /** Drop the proposal without applying anything. */
  discard() {
    this.proposal = null;
    if (this.status === 'review') this.status = 'idle';
  }

  async #run(instruction, start) {
    const path = this.#documents.active?.path ?? null;
    if (!path) {
      this.#ui.toast('Open a file first', 'error');
      return;
    }
    if (this.running) return;

    this.messages.push({ id: id(), role: 'user', text: instruction });
    this.messages.push({ id: id(), role: 'assistant', run: { steps: [], outcome: null, note: '' } });
    // Read back through the reactive array so later edits are seen by the UI.
    const run = this.messages.at(-1).run;
    this.proposal = null;
    this.status = 'running';
    this.#ui.setStatus('AI is working…');
    this.#abort = new AbortController();
    const { signal } = this.#abort;

    const timeline = new Timeline(run);
    try {
      await start(
        path,
        (message) => this.#onEvent(message, run, timeline, path),
        (error) => timeline.fail(error.message),
        signal
      );
      if (signal.aborted) {
        timeline.cancelled();
      }
    } catch (e) {
      // The call itself rejected: it never connected, or the server answered
      // with an error envelope instead of a stream.
      timeline.fail(e.message);
      this.#ui.toast(`AI request failed: ${e.message}`, 'error');
    } finally {
      timeline.close();
      if (run.outcome === null) run.outcome = this.proposal ? OUTCOME.review : OUTCOME.failed;
      this.status = this.proposal ? 'review' : 'idle';
      this.#abort = null;
      this.#ui.setStatus('Ready');
    }
  }

  #onEvent({ event, data }, run, timeline, path) {
    switch (event) {
      case 'thinking':
        timeline.begin('thinking', `Attempt ${data.iteration}`, data.message ?? 'working out the edits');
        break;
      case 'step':
        timeline.update(`Attempt ${data.iteration}: ${plural(data.edit_count, 'edit')} proposed`);
        break;
      case 'progress':
        timeline.note(data.message ?? 'working…');
        break;
      case 'applying':
        timeline.begin('applying', 'Trying the edits', data.message);
        break;
      case 'compiled':
        if (data.success) timeline.finish('ok', 'It compiles');
        else timeline.finish('failed', 'It did not compile', data.error_summary);
        break;
      case 'proposal': {
        const files = data.files?.length ?? 1;
        timeline.add(
          'proposal',
          `Proposed ${plural(data.edit_count, 'edit')}${files > 1 ? ` in ${files} files` : ''}`,
          'ok'
        );
        this.proposal = { ...data, file: path };
        run.outcome = OUTCOME.review;
        this.#ui.show('chat');
        break;
      }
      case 'awaiting_approval':
        run.outcome = OUTCOME.review;
        break;
      case 'done':
        if (run.outcome === null) {
          run.outcome = data.success ? OUTCOME.success : OUTCOME.failed;
          if (!data.success && data.message) run.note = data.message;
        }
        break;
      case 'error':
        timeline.fail(data.message);
        break;
      default:
        break;
    }
  }

  /**
   * Apply the hunks the user accepted. The write is conditional on the file
   * still being what the proposal was made against; if it moved and the server
   * still holds the proposal, the hunks are moved onto the new text (the server
   * does so only when every one still fits, else it answers 409 again).
   */
  async apply({ edits, selectedHunkIds, expectedEtag } = {}) {
    if (!edits?.length) {
      this.#ui.toast('No proposal to apply', 'error');
      return false;
    }
    if (!selectedHunkIds?.length) {
      this.#ui.toast('No hunks selected', 'info');
      return false;
    }
    const proposal = this.proposal;
    const filePath = proposal?.file ?? this.#documents.active?.path;
    const proposalId = proposal?.proposal_id ?? null;
    const base = proposal?.etag ?? expectedEtag;

    this.status = 'applying';
    this.#ui.setStatus('Applying selected edits…');
    try {
      let res;
      try {
        res = await this.#service.applySelectedHunks(filePath, edits, selectedHunkIds, base, { proposalId });
      } catch (err) {
        if (err.status !== 409 || !proposalId) throw err;
        res = await this.#service.applySelectedHunks(filePath, edits, selectedHunkIds, base, {
          proposalId,
          rebase: true
        });
        this.#ui.toast('The file changed while you reviewed; your changes were moved onto it', 'info');
      }
      if (!res?.success) throw new Error(res?.error || 'Unknown error');

      // The server wrote the files; re-read every one that is open so the
      // editor and the disk cannot drift apart.
      const written = new Set([filePath, ...(res.files ?? []).map((f) => f.path)]);
      for (const p of written) {
        if (this.#documents.get(p)) await this.#documents.reload(p);
      }
      await this.#project.refresh();

      this.proposal = null;
      this.status = 'idle';
      this.#ui.setStatus('Selected edits applied');
      this.#ui.toast('Selected changes applied', 'success');
      this.#compile.start(filePath, { main: this.#project.mainOverride });
      return true;
    } catch (err) {
      this.status = this.proposal ? 'review' : 'idle';
      // 409: the file moved under the review. Say so specifically — a generic
      // failure would invite the user to retry into the same conflict.
      if (err.status === 409) {
        this.#ui.setStatus('File changed during review');
        this.#ui.toast('File changed while you were reviewing — reload and re-plan', 'error');
      } else {
        this.#ui.setStatus(`Apply failed: ${err.message}`);
        this.#ui.toast(`Apply failed: ${err.message}`, 'error');
      }
      return false;
    }
  }
}

const plural = (n, word) => `${n} ${word}${n === 1 ? '' : 's'}`;

/**
 * Builds a run's steps. One step is "active" at a time; starting another
 * closes it as done, so the list always reads as a sequence.
 */
class Timeline {
  #run;
  constructor(run) {
    this.#run = run;
  }

  get #active() {
    return this.#run.steps.findLast((s) => s.state === 'active') ?? null;
  }

  add(kind, text, state, detail = '') {
    this.#run.steps.push({ id: id(), kind, text, detail, state });
  }

  begin(kind, text, detail = '') {
    this.finish('ok');
    this.add(kind, text, 'active', detail ?? '');
  }

  /** Close the active step, optionally changing its text and detail. */
  finish(state, text, detail) {
    const step = this.#active;
    if (!step) {
      if (text) this.add('result', text, state, detail ?? '');
      return;
    }
    step.state = state;
    if (text) step.text = text;
    if (detail !== undefined) step.detail = detail;
  }

  update(text) {
    const step = this.#active;
    if (step) step.text = text;
    else this.add('thinking', text, 'active');
  }

  note(detail) {
    const step = this.#active;
    if (step) step.detail = detail;
    else this.add('note', detail, 'ok');
  }

  fail(message) {
    this.finish('failed');
    this.add('error', message, 'failed');
    this.#run.outcome ??= OUTCOME.failed;
    this.#run.note = message;
  }

  cancelled() {
    this.finish('failed', 'Stopped');
    this.#run.outcome = OUTCOME.cancelled;
  }

  close() {
    this.finish('ok');
  }
}
