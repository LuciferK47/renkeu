// AI store: manages AI agent state (proposal, diffs, messages, session)
import { writable } from 'svelte/store'
import { rafThrottle } from '../utils/debounce.js'

/**
 * Generate or retrieve persistent session ID
 * Sessions persist across browser reloads to maintain conversation context
 */
function getOrCreateSessionId() {
  const STORAGE_KEY = 'latex_ai_session_id'
  let sessionId = localStorage.getItem(STORAGE_KEY)
  
  if (!sessionId) {
    // Generate new session ID: timestamp + random
    sessionId = `session_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`
    localStorage.setItem(STORAGE_KEY, sessionId)
  }
  
  return sessionId
}

export const aiStore = writable({
  sessionId: getOrCreateSessionId(),
  messages: [],
  currentProposal: null, // {summary, edits, diffs, iteration} - proposal awaiting user approval
  diffs: [], // [{file_path, diff, hunks: [{+lines, -lines, context}]}]
  suggestions: [],
  codeReview: null,
  status: 'idle', // idle, thinking, streaming, ready, applying, error, proposal (awaiting user)
  error: null,
  streaming: false,
  streamingMessage: '',
  autoApply: false, // If true, skip proposal approval and apply automatically
  proposalIteration: 0 // Current iteration number for multi-turn agent loop
})

/**
 * Reset to a new session (clears conversation history)
 */
export function startNewSession() {
  const STORAGE_KEY = 'latex_ai_session_id'
  const newSessionId = `session_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`
  localStorage.setItem(STORAGE_KEY, newSessionId)
  
  aiStore.set({
    sessionId: newSessionId,
    messages: [],
    currentProposal: null,
    diffs: [],
    suggestions: [],
    codeReview: null,
    status: 'idle',
    error: null,
    streaming: false,
    streamingMessage: '',
    autoApply: false,
    proposalIteration: 0
  })
}

export function initSession(sessionId) {
  aiStore.update(a => ({
    ...a,
    sessionId: sessionId || getOrCreateSessionId(),
    messages: [],
    currentProposal: null,
    diffs: [],
    suggestions: [],
    codeReview: null,
    error: null
  }))
}


export function addMessage(role, content) {
  aiStore.update(a => ({
    ...a,
    messages: [...a.messages, { role, content, timestamp: new Date() }]
  }))
}

export function startStreaming(prompt) {
  aiStore.update(a => ({
    ...a,
    status: 'streaming',
    error: null,
    streaming: true,
    streamingMessage: ''
  }))
}

/**
 * Update streaming message with debouncing to reduce DOM thrashing.
 * Uses requestAnimationFrame to batch updates at 60fps max.
 */
const debouncedUpdate = rafThrottle((chunk) => {
  aiStore.update(a => ({
    ...a,
    streamingMessage: a.streamingMessage + chunk
  }))
})

export function updateStreamingMessage(chunk) {
  debouncedUpdate(chunk)
}

/**
 * Set current proposal from agent loop and pause for user approval.
 * @param {Object} proposal - {summary, edits, diffs, iteration}
 */
export function setProposal(proposal) {
  aiStore.update(a => ({
    ...a,
    currentProposal: proposal,
    diffs: proposal.diffs || [],
    proposalIteration: proposal.iteration || 0,
    status: a.autoApply ? 'applying' : 'proposal', // If autoApply, continue; else wait for user
    streaming: a.autoApply, // Resume streaming if autoApply
    streamingMessage: ''
  }))
}

/**
 * Clear the current proposal after user accepts or rejects.
 */
export function clearProposal() {
  aiStore.update(a => ({
    ...a,
    currentProposal: null,
    diffs: [],
    status: 'idle'
  }))
}

/**
 * Toggle auto-apply mode to skip proposal approval step.
 */
export function setAutoApply(enabled) {
  aiStore.update(a => ({
    ...a,
    autoApply: enabled
  }))
}

export function setSuggestions(suggestions) {
  aiStore.update(a => ({
    ...a,
    suggestions,
    status: 'ready'
  }))
}

export function setCodeReview(review) {
  aiStore.update(a => ({
    ...a,
    codeReview: review,
    status: 'ready'
  }))
}

export function setApplying() {
  aiStore.update(a => ({...a, status: 'applying'}))
}

export function setError(error) {
  aiStore.update(a => ({
    ...a,
    status: 'error',
    error,
    streaming: false,
    streamingMessage: ''
  }))
}

export function resetAI() {
  // Keep sessionId persistent across resets (only cleared by startNewSession)
  aiStore.update(a => ({
    ...a,
    messages: [],
    currentProposal: null,
    diffs: [],
    suggestions: [],
    codeReview: null,
    status: 'idle',
    error: null,
    streaming: false,
    streamingMessage: '',
    autoApply: false,
    proposalIteration: 0
  }))
}
