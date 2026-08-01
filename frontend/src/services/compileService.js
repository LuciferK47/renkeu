/**
 * Compile Service — start a compile job and follow it over SSE.
 * Maps to crates/latex_ai_server/src/routes/compile.rs.
 */

import { api, APIError } from '../lib/api/client.js';
import { streamSSE } from './_stream.js';

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
export async function streamCompilationStatus(jobId, onEvent, onError) {
  try {
    const response = await fetch(api.compileStreamUrl(jobId));
    if (!response.ok) throw new Error(`compile stream failed (${response.status})`);
    for await (const message of streamSSE(response)) onEvent(message);
  } catch (error) {
    if (onError) onError(error);
    else throw error;
  }
}

export function pdfPageUrl(filePath, page, dpi) {
  return api.pdfPageUrl(filePath, page, dpi);
}
