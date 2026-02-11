// Compatibility layer that forwards to the consolidated API client
import { api } from './lib/api/client.js'

// Generic request helpers
export async function apiRequest(endpoint, options = {}) {
  const { method = 'GET', body, headers, params, signal } = options
  return api.request(method, endpoint, { body, headers, params, signal })
}

export async function apiGet(endpoint, options = {}) {
  return apiRequest(endpoint, { ...options, method: 'GET' })
}

export async function apiPost(endpoint, data, options = {}) {
  return apiRequest(endpoint, { ...options, method: 'POST', body: data })
}

export async function apiPut(endpoint, data, options = {}) {
  return apiRequest(endpoint, { ...options, method: 'PUT', body: data })
}

export async function apiDelete(endpoint, options = {}) {
  return apiRequest(endpoint, { ...options, method: 'DELETE' })
}

export function apiStream(endpoint, data, onEvent, onError) {
  const base = api.baseURL || '/api'
  const url = new URL(`${base}${endpoint}`, window.location.origin)
  if (data) {
    Object.entries(data).forEach(([key, value]) => {
      if (value === undefined || value === null) return
      url.searchParams.append(key, typeof value === 'object' ? JSON.stringify(value) : value)
    })
  }
  const es = new EventSource(url)
  es.addEventListener('message', (e) => {
    try {
      onEvent('message', JSON.parse(e.data))
    } catch {
      onEvent('message', e.data)
    }
  })
  es.addEventListener('error', (e) => {
    es.close()
    if (onError) onError(new Error('Stream connection lost'))
  })
  return () => es.close()
}

export const files = {
  list: (limit = 40, offset = 0, search = '') =>
    apiRequest('/files', { method: 'GET', params: { limit, offset, search } }),
  get: (path) => api.getFile(path),
  save: (path, content, etag = null) => api.saveFile(path, content, etag),
  delete: (path) => apiRequest('/file', { method: 'DELETE', params: { path } }),
  recentFiles: () => apiRequest('/files/recent', { method: 'GET' }),
  pinFile: (path) => apiRequest('/files/pin', { method: 'POST', body: { path } }),
  unpinFile: (path) => apiRequest('/files/pin', { method: 'DELETE', params: { path } }),
}

export const agent = {
  propose: (sessionId, plan) =>
    apiRequest('/agent/proposal', { method: 'POST', body: { session_id: sessionId, plan } }),
  apply: (sessionId, proposalId, edits) =>
    apiRequest('/agent/apply', { method: 'POST', body: { session_id: sessionId, proposal_id: proposalId, edits } }),
  createPlan: (sessionId, prompt, files) =>
    apiRequest('/plan', { method: 'POST', body: { session_id: sessionId, prompt, files } }),
  streamPlan: (sessionId, prompt, files, onEvent, onError) =>
    apiStream('/plan/stream', { session_id: sessionId, prompt, files }, onEvent, onError),
  undo: (sessionId) =>
    apiRequest('/agent/undo', { method: 'POST', body: { session_id: sessionId } }),
  redo: (sessionId) =>
    apiRequest('/agent/redo', { method: 'POST', body: { session_id: sessionId } }),
}

export const config = {
  get: () => api.getConfig(),
  set: (data) => api.setConfig(data),
  getModels: () => apiRequest('/models', { method: 'GET' }),
}

export const compile = {
  compile: (files, options = {}) => apiRequest('/compile', { method: 'POST', body: { files, ...options } }),
  status: (jobId) => api.getCompileStatus(jobId),
}

export const history = {
  get: (sessionId, limit = 50) => apiRequest('/history', { method: 'GET', params: { session_id: sessionId, limit } }),
  agentHistory: (sessionId, limit = 50) => apiRequest('/history/agent', { method: 'GET', params: { session_id: sessionId, limit } }),
  clear: (sessionId) => apiRequest('/history', { method: 'DELETE', params: { session_id: sessionId } }),
}
