/**
 * AI Service - Handles all AI-related operations
 * - Request AI planning
 * - Stream AI responses
 * - Handle suggestions
 * - Code review streaming
 */

import { api } from '../lib/api/client.js'

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

    if (!response.ok) {
      throw new Error('AI stream failed')
    }

    const reader = response.body.getReader()
    const decoder = new TextDecoder()
    let buffer = ''

    while (true) {
      const { done, value } = await reader.read()
      if (done) {
        if (onComplete) onComplete()
        break
      }

      buffer += decoder.decode(value)
      const lines = buffer.split('\n')
      buffer = lines.pop() || ''

      for (const line of lines) {
        if (line.startsWith('data: ')) {
          const payload = line.slice(6)
          if (!payload.trim()) continue
          try {
            onProgress(JSON.parse(payload))
          } catch (e) {
            console.error('Failed to parse AI stream:', e)
          }
        }
      }
    }
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

    const reader = response.body.getReader()
    const decoder = new TextDecoder()
    let buffer = ''
    let currentEvent = 'message'

    while (true) {
      const { done, value } = await reader.read()
      if (done) {
        if (onComplete) onComplete()
        break
      }

      buffer += decoder.decode(value)
      const lines = buffer.split('\n')
      buffer = lines.pop() || ''

      for (const line of lines) {
        if (line.startsWith('event: ')) {
          currentEvent = line.slice(7).trim()
        } else if (line.startsWith('data: ')) {
          const payload = line.slice(6)
          if (!payload.trim()) continue
          try {
            const data = JSON.parse(payload)
            onProgress({ event: currentEvent, data })
          } catch (e) {
            console.error('Failed to parse suggestion stream:', e)
          }
        }
      }
    }
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

    const reader = response.body.getReader()
    const decoder = new TextDecoder()
    let buffer = ''
    let currentEvent = 'message'

    while (true) {
      const { done, value } = await reader.read()
      if (done) {
        if (onComplete) onComplete()
        break
      }

      buffer += decoder.decode(value)
      const lines = buffer.split('\n')
      buffer = lines.pop() || ''

      for (const line of lines) {
        if (line.startsWith('event: ')) {
          currentEvent = line.slice(7).trim()
        } else if (line.startsWith('data: ')) {
          const payload = line.slice(6)
          if (!payload.trim()) continue
          try {
            const data = JSON.parse(payload)
            onProgress({ event: currentEvent, data })
          } catch (e) {
            console.error('Failed to parse review stream:', e)
          }
        }
      }
    }
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

    const reader = response.body.getReader()
    const decoder = new TextDecoder()
    let buffer = ''
    let currentEvent = 'message'

    while (true) {
      const { done, value } = await reader.read()
      if (done) {
        if (onComplete) onComplete()
        break
      }

      buffer += decoder.decode(value)
      const lines = buffer.split('\n')
      buffer = lines.pop() || ''

      for (const line of lines) {
        if (line.startsWith('event: ')) {
          currentEvent = line.slice(7).trim()
        } else if (line.startsWith('data: ')) {
          const payload = line.slice(6)
          if (!payload.trim()) continue
          try {
            const data = JSON.parse(payload)
            onProgress({ event: currentEvent, data })
          } catch (e) {
            console.error('Failed to parse explain stream:', e)
          }
        }
      }
    }
  } catch (error) {
    if (onError) onError(error)
  }
}
