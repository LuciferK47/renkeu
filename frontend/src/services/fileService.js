/**
 * File Service - Handles all file operations
 * - Load file content
 * - Save file content
 * - Handle conflicts and ETags
 * - Manage file metadata
 */

import { api, APIError } from '../lib/api/client.js'

export async function loadFile(filePath) {
  if (!filePath) throw new Error('File path required')
  const data = await api.getFile(filePath)
  return {
    content: data.content,
    path: filePath,
    lastModified: data.modified_at,
    eTag: data.etag
  }
}

export async function saveFile(filePath, content, eTag = null) {
  if (!filePath) throw new Error('File path required')
  try {
    return await api.saveFile(filePath, content, eTag)
  } catch (err) {
    if (err instanceof APIError && err.status === 409) {
      const conflict = new Error('File conflict: file modified elsewhere')
      conflict.status = 409
      conflict.conflictData = err.payload
      throw conflict
    }
    throw err
  }
}

export async function applyCommand(filePath, command, dryRun = false) {
  return await api.request('POST', '/apply', {
    body: { file_path: filePath, command, dry_run: dryRun }
  })
}

export async function applyPatch(filePath, hunks) {
  return await api.request('POST', '/file', {
    body: { path: filePath, hunks }
  })
}

export async function getFileHistory(filePath, limit = 50) {
  return await api.request('GET', '/history', {
    params: { file_path: filePath, limit }
  })
}

export async function restoreFileVersion(filePath, versionId) {
  return await api.request('POST', `/history/${versionId}/restore`, {
    body: { file_path: filePath }
  })
}

  export async function restoreHistoryEntry(historyId, filePath) {
    return await api.request('POST', `/restore`, {
      body: { history_id: historyId, file_path: filePath }
    })
  }

export async function bulkApply(files, command) {
  try {
    return await api.request('POST', '/bulk-apply', {
      body: { files, command }
    })
  } catch (err) {
    if (err instanceof APIError) {
      const e = new Error(err.payload?.detail || 'Bulk apply failed')
      e.status = err.status
      throw e
    }
    throw err
  }
}
