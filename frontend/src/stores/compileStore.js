// Enhanced compile store with debounced auto-compile
import { writable } from 'svelte/store'
import { debounce } from '../utils/debounce.js'

export const compileStore = writable({
  jobId: null,
  status: 'idle', // idle, pending, compiling, success, error
  progress: 0,
  pdfUrl: null,
  pageCount: 0,
  errors: [],
  warnings: [],
  logs: [],
  duration: null,
  lastCompileAt: null,
  autoCompile: true,
  autoCompileDelay: 2000, // 2 second delay for auto-compile
  pendingAutoCompile: false // Indicates debounced compile is pending
})

export function setCompileJob(jobId, status = 'pending') {
  compileStore.update(c => ({
    ...c,
    jobId,
    status,
    progress: 0,
    errors: [],
    warnings: [],
    logs: [],
    pendingAutoCompile: false
  }))
}

export function updateCompileProgress(progress, logs = []) {
  compileStore.update(c => ({
    ...c,
    progress: Math.min(progress, 100),
    logs: [...(c.logs || []), ...logs]
  }))
}

export function setCompileSuccess(result) {
  compileStore.update(c => ({
    ...c,
    jobId: null,
    status: 'success',
    progress: 100,
    pdfUrl: result.pdf_url,
    pageCount: result.page_count || 0,
    errors: [],
    warnings: result.warnings || [],
    logs: [],
    duration: result.compile_time,
    lastCompileAt: new Date(),
    pendingAutoCompile: false
  }))
}

export function setCompileError(errors, warnings = []) {
  compileStore.update(c => ({
    ...c,
    jobId: null,
    status: 'error',
    progress: 0,
    pdfUrl: null,
    pageCount: 0,
    errors,
    warnings,
    logs: [],
    duration: null,
    lastCompileAt: new Date(),
    pendingAutoCompile: false
  }))
}

export function resetCompile() {
  compileStore.update(c => ({
    ...c,
    jobId: null,
    status: 'idle',
    progress: 0,
    pdfUrl: null,
    pageCount: 0,
    errors: [],
    warnings: [],
    logs: [],
    duration: null,
    lastCompileAt: null,
    pendingAutoCompile: false
  }))
}

/**
 * Set auto-compile delay in milliseconds.
 * Adjusts debounce delay for auto-compile triggers.
 */
export function setAutoCompileDelay(delay) {
  compileStore.update(c => ({
    ...c,
    autoCompileDelay: Math.max(500, Math.min(delay, 10000)) // Clamp between 500ms and 10s
  }))
}

/**
 * Mark that an auto-compile is pending (debounced).
 * Shows visual indicator to user that compile will trigger soon.
 */
export function setPendingAutoCompile(pending) {
  compileStore.update(c => ({
    ...c,
    pendingAutoCompile: pending
  }))
}

/**
 * Create a debounced compile function that can be cancelled.
 * Returns a function that triggers compilation after the specified delay.
 * 
 * @param {Function} compileCallback - Function to call when compile should execute
 * @param {number} delay - Debounce delay in milliseconds (default: 2000)
 * @returns {Function} Debounced compile function with cancel method
 */
export function createDebouncedCompile(compileCallback, delay = 2000) {
  const debouncedCompile = debounce((filePath) => {
    setPendingAutoCompile(false)
    compileCallback(filePath)
  }, delay)
  
  return {
    trigger: (filePath) => {
      setPendingAutoCompile(true)
      debouncedCompile(filePath)
    },
    cancel: () => {
      setPendingAutoCompile(false)
      debouncedCompile.cancel()
    }
  }
}
