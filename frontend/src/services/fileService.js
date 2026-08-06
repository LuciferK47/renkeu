/**
 * File Service — read/save workspace files with ETag conflict detection.
 * Maps to crates/latex_ai_server/src/routes/files.rs.
 */

import { api, APIError } from '../lib/api/client.js';

export async function loadFile(filePath) {
  if (!filePath) throw new Error('File path required');
  const data = await api.getFile(filePath);
  return {
    content: data.content,
    path: filePath,
    lastModified: data.modified_at,
    eTag: data.etag
  };
}

export async function saveFile(filePath, content, eTag = null) {
  if (!filePath) throw new Error('File path required');
  try {
    return await api.saveFile(filePath, content, eTag);
  } catch (err) {
    if (err instanceof APIError && err.status === 409) {
      const conflict = new Error('File conflict: file modified elsewhere');
      conflict.status = 409;
      // Envelope carries the winning etag so the UI can offer "reload".
      conflict.currentEtag = err.payload?.current_etag ?? null;
      conflict.conflictData = err.payload;
      throw conflict;
    }
    throw err;
  }
}

export async function listFiles() {
  const data = await api.listFiles();
  return data.files || [];
}

export async function searchWorkspace(query) {
  const data = await api.searchWorkspace(query);
  return data.hits || [];
}
