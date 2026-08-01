/**
 * AI Service — plan/agent streaming and hunk application.
 * Request/response shapes mirror crates/latex_ai_server/src/routes/agent.rs.
 */

import { api } from '../lib/api/client.js';
import { streamSSE } from './_stream.js';

/**
 * Stream a one-shot plan. `onEvent` receives { event, data } from the locked
 * §8.2 taxonomy: progress -> proposal -> done | error.
 */
export async function streamAIPlan(filePath, instruction, onEvent, onError) {
  return streamAgentEndpoint('/plan/stream', { file_path: filePath, instruction }, onEvent, onError);
}

/**
 * Stream the self-healing agent loop.
 * Events: thinking -> proposal -> (awaiting_approval | applying -> compiled -> done).
 */
export async function agentLoopStream(filePath, instruction, onEvent, onError, options = {}) {
  const body = {
    file_path: filePath,
    instruction,
    auto_apply: options.autoApply ?? false
  };
  if (options.maxIterations) body.max_iterations = options.maxIterations;
  return streamAgentEndpoint('/agent/stream', body, onEvent, onError);
}

async function streamAgentEndpoint(path, body, onEvent, onError) {
  try {
    const response = await fetch(api.buildUrl(path), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body)
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

    for await (const message of streamSSE(response)) onEvent(message);
  } catch (error) {
    if (onError) onError(error);
    else throw error;
  }
}

export async function previewHunks(filePath, edits) {
  return api.previewHunks(filePath, edits);
}

export async function applySelectedHunks(filePath, edits, selectedHunkIds, expectedEtag = null) {
  return api.applySelectedHunks(filePath, edits, selectedHunkIds, expectedEtag);
}

export async function requestVisualFix(region) {
  return api.repairVisualCrop(region);
}
