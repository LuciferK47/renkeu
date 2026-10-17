/**
 * Compile Service — start a compile job and follow it over SSE.
 * Maps to crates/latex_ai_server/src/routes/compile.rs.
 */

import { api, APIError } from '../lib/api/client.js';
import { consumeSSE } from './_stream.js';

/** -> { job_id, status } */
export async function startCompilation(filePath, signal) {
  try {
    return await api.compile(filePath, signal);
  } catch (err) {
    if (err instanceof APIError) throw new Error(err.message || 'Compilation failed');
    throw err;
  }
}

/**
 * Follow a compile job. `onEvent` receives { event, data } from the taxonomy:
 * progress* -> compiled -> done (or error).
 */
/** Attempts before giving up on a stream that keeps dropping. */
const MAX_ATTACH_ATTEMPTS = 3;

export async function streamCompilationStatus(jobId, onEvent, onError, options = {}) {
  const maxAttempts = options.maxAttempts ?? MAX_ATTACH_ATTEMPTS;
  const sleep = options.sleep ?? ((ms) => new Promise((r) => setTimeout(r, ms)));

  // Unlike the agent stream, re-attaching here is safe: the job already exists
  // server-side and this endpoint only observes it, so a retry cannot start a
  // second compile or apply anything twice.
  for (let attempt = 1; attempt <= maxAttempts; attempt++) {
    try {
      const response = await fetch(api.compileStreamUrl(jobId));
      if (!response.ok) throw new Error(`compile stream failed (${response.status})`);

      const finished = await consumeSSE(response, onEvent);
      if (finished) return;

      if (attempt === maxAttempts) {
        throw new Error('Lost contact with the compile job. It may have finished — reload to check.');
      }
    } catch (error) {
      if (attempt === maxAttempts) {
        if (onError) return onError(error);
        throw error;
      }
    }
    // Back off before re-attaching, so a server that is briefly unreachable is
    // not hammered: 250ms, 500ms.
    await sleep(250 * 2 ** (attempt - 1));
  }
}

export function pdfPageUrl(filePath, page, dpi, buildId) {
  return api.pdfPageUrl(filePath, page, dpi, undefined, buildId);
}
