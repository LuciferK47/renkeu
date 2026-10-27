/**
 * AI Service — plan/agent streaming and hunk application.
 * Request/response shapes mirror crates/latex_ai_server/src/routes/agent.rs.
 */

import { api } from '../lib/api/client.js';
import { consumeSSE } from './_stream.js';

/**
 * Stream a one-shot plan. `onEvent` receives { event, data } from the locked
 * §8.2 taxonomy: progress -> proposal -> done | error.
 */
async function streamAIPlan(filePath, instruction, onEvent, onError, options = {}) {
  return streamAgentEndpoint(
    '/plan/stream',
    { file_path: filePath, instruction },
    onEvent,
    onError,
    options.signal
  );
}

/**
 * Stream the self-healing agent loop.
 * Events: thinking -> proposal -> (awaiting_approval | applying -> compiled -> done).
 */
async function agentLoopStream(filePath, instruction, onEvent, onError, options = {}) {
  const body = {
    file_path: filePath,
    instruction,
    auto_apply: options.autoApply ?? false
  };
  if (options.maxIterations) body.max_iterations = options.maxIterations;
  // Fit the document to this many pages; the server measures every attempt.
  if (options.pages) body.pages = options.pages;
  return streamAgentEndpoint('/agent/stream', body, onEvent, onError, options.signal);
}

/**
 * `signal` lets the caller stop a run. Aborting closes the connection, which is
 * what the server watches for: it cancels the planner call and any compile
 * bound to the run. An aborted stream is the caller's own doing, so it is
 * neither reported through `onError` nor thrown.
 */
async function streamAgentEndpoint(path, body, onEvent, onError, signal) {
  try {
    const response = await fetch(api.buildUrl(path), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
      signal
    });

    if (!response.ok) {
      // Errors arrive as the JSON envelope, not as a stream.
      let detail = response.statusText;
      try {
        const payload = await response.json();
        detail = payload.message || payload.error || detail;
      } catch {
        /* keep statusText */
      }
      throw new Error(detail);
    }

    const finished = await consumeSSE(response, onEvent);
    if (!finished) {
      // Deliberately not retried. Re-POSTing this endpoint starts a *second*
      // agent run against the same file, and with auto_apply that can apply the
      // same edits twice. The run may still be going server-side, so say that
      // rather than implying it failed.
      throw new Error('Connection lost before the agent finished. It may still be running — reload to see the current state.');
    }
  } catch (error) {
    if (error?.name === 'AbortError') return;
    if (onError) onError(error);
    else throw error;
  }
}

async function applySelectedHunks(
  filePath,
  edits,
  selectedHunkIds,
  expectedEtag = null,
  options = {}
) {
  return api.applySelectedHunks(filePath, edits, selectedHunkIds, expectedEtag, null, options);
}

/** What `AiState` talks to; tests pass a stand-in with the same three methods. */
export const aiService = { agentLoopStream, streamAIPlan, applySelectedHunks };
