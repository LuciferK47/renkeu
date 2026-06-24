/**
 * Compile Service - Handles all PDF compilation operations
 * - Trigger compilation
 * - Track compilation status
 * - Check cache
 * - Handle compilation results
 */

import { api, APIError } from '../lib/api/client.js'
import { streamSSE } from './_stream.js'

export async function checkCompileCache(filePath, outputDir = null, signal) {
  return api.checkCompileCache(filePath, outputDir, signal)
}

export async function startCompilation(filePath, engine = 'pdflatex', signal) {
  try {
    return await api.compile(filePath, engine, signal)
  } catch (err) {
    if (err instanceof APIError) {
      throw new Error(err.payload?.detail || 'Compilation failed')
    }
    throw err
  }
}

export async function getCompilationStatus(jobId, signal) {
  return api.getCompileStatus(jobId, signal)
}

export async function cancelCompilation(jobId) {
  // Backend may not support cancel; fall back to delete if available
  return api.request('DELETE', `/compile/${jobId}`)
}

export async function streamCompilationStatus(jobId, onProgress, onComplete, onError) {
  try {
    const url = api.buildUrl(`/compile/stream/${jobId}`)
    const response = await fetch(url)
    if (!response.ok) throw new Error('Stream failed')

    for await (const { data } of streamSSE(response)) onProgress(data)
    if (onComplete) onComplete()
  } catch (error) {
    if (onError) onError(error)
  }
}

export function getPDF(jobId) {
  return api.getPdfUrl(jobId)
}

export async function getSyncTeXInfo(filePath, page, h, v) {
  return api.syncTex(filePath, page, h, v)
}
