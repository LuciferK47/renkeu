// Compatibility shim that re-exports the consolidated API client
import { api, APIClient, APIError } from './lib/api/client.js'

// Re-export primary client & error
export { api, APIClient, APIError }

// Convenience wrappers using the consolidated client
export const filesAPI = {
  list: (params) => api.request('GET', '/files', { params }),
  get: (path) => api.getFile(path),
  save: (path, content, etag) => api.saveFile(path, content, etag),
  create: (path, content) => api.request('POST', '/file', { body: { path, content } }),
  delete: (path) => api.request('DELETE', '/file', { params: { path } }),
}

export const agentAPI = {
  propose: (request, etag) => api.request('POST', '/agent/proposal', { body: { ...request, etag } }),
  apply: (proposalId, etag) => api.request('POST', '/agent/apply', { body: { proposal_id: proposalId, etag } }),
  undo: () => api.request('POST', '/agent/undo', { body: {} }),
  redo: () => api.request('POST', '/agent/redo', { body: {} }),
  history: (agentOnly = false) => api.request('GET', '/agent/history', { params: { agent_only: agentOnly } }),
}

export const planAPI = {
  create: (goal, context) => api.request('POST', '/plan', { body: { goal, context } }),
  stream: (goal, context, sessionId, onEvent, onError) => {
    const url = new URL(api.baseURL ? `${api.baseURL}/plan/stream` : '/api/plan/stream', window.location.origin)
    url.searchParams.set('goal', goal)
    if (context) url.searchParams.set('context', JSON.stringify(context))
    if (sessionId) url.searchParams.set('session_id', sessionId)
    const es = new EventSource(url)
    es.onmessage = (e) => {
      try { onEvent(JSON.parse(e.data)) } catch { onEvent(e.data) }
    }
    es.onerror = (err) => {
      es.close()
      if (onError) onError(err)
    }
    return () => es.close()
  },
}

export const compileAPI = {
  compile: (mainFile, engine) => api.compile(mainFile, engine),
  status: (jobId) => api.getCompileStatus(jobId),
}

export const configAPI = {
  get: () => api.getConfig(),
  update: (config) => api.setConfig(config),
}
