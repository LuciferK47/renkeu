/**
 * Compile Service - Handles all PDF compilation operations
 * - Trigger compilation
 * - Track compilation status
 * - Check cache
 * - Handle compilation results
 */

import { api, APIError } from '../lib/api/client.js'

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

    const reader = response.body.getReader()
    const decoder = new TextDecoder()
    while (true) {
      const { done, value } = await reader.read()
      if (done) {
        if (onComplete) onComplete()
        break
      }
      const text = decoder.decode(value)
      const lines = text.split('\n')
      for (const line of lines) {
        if (line.startsWith('data: ')) {
          const payload = line.slice(6)
          if (!payload.trim()) continue
          try { onProgress(JSON.parse(payload)) } catch (e) { console.error('Stream parse error', e) }
        }
      }
    }
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
