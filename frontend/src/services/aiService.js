/**
 * AI Service - Handles all AI-related operations
 * - Request AI planning
 * - Stream AI responses
 * - Handle suggestions
 * - Code review streaming
 */

import { api } from '../lib/api/client.js'
import { streamSSE } from './_stream.js'

export async function requestAIPlan(filePath, requestText, model = null) {
  return api.request('POST', '/plan', {
    body: {
      file_path: filePath,
      request_text: requestText,
      model: model || undefined
    }
  })
}

export async function streamAIPlan(
  filePath,
  requestText,
  context = [],
  sessionId = null,
  onProgress,
  onComplete,
  onError
) {
  try {
    const response = await fetch(api.buildUrl('/plan/stream'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        file_path: filePath || 'main.tex',
        request_text: requestText,
        context: context.map(m => ({ role: m.role, content: m.content })),
        session_id: sessionId
      })
    })
    if (!response.ok) throw new Error('AI stream failed')

    for await (const { data } of streamSSE(response)) onProgress(data)
    if (onComplete) onComplete()
  } catch (error) {
    if (onError) onError(error)
  }
}

export async function streamSuggestions(
  filePath,
  sessionId = null,
  context = [],
  onProgress,
  onComplete,
  onError
) {
  try {
    const response = await fetch(api.buildUrl('/suggest/stream'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        file_path: filePath,
        session_id: sessionId,
        context: context.map(m => ({ role: m.role, content: m.content }))
      })
    })
    if (!response.ok) throw new Error('Suggestions request failed')

    for await (const evt of streamSSE(response)) onProgress(evt)
    if (onComplete) onComplete()
  } catch (error) {
    if (onError) onError(error)
  }
}

export async function streamCodeReview(
  filePath,
  sessionId = null,
  onProgress,
  onComplete,
  onError
) {
  try {
    const response = await fetch(api.buildUrl('/review/stream'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ file_path: filePath, session_id: sessionId })
    })
    if (!response.ok) throw new Error('Code review request failed')

    for await (const evt of streamSSE(response)) onProgress(evt)
    if (onComplete) onComplete()
  } catch (error) {
    if (onError) onError(error)
  }
}

export async function getAvailableModels() {
  return api.request('GET', '/models')
}

export async function streamExplainErrors(
  filePath,
  diagnostics,
  context = [],
  sessionId = null,
  onProgress,
  onComplete,
  onError
) {
  try {
    const response = await fetch(api.buildUrl('/errors/explain/stream'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        file_path: filePath || 'main.tex',
        diagnostics,
        session_id: sessionId,
        context: context.map(m => ({ role: m.role, content: m.content }))
      })
    })
    if (!response.ok) throw new Error('AI error explanation request failed')

    for await (const evt of streamSSE(response)) onProgress(evt)
    if (onComplete) onComplete()
  } catch (error) {
    if (onError) onError(error)
  }
}
