/**
 * Error handling and user-friendly error messages for the frontend
 */

/**
 * Parse API error response
 * @param {Error|Response} error - Error object or fetch response
 * @returns {object} {status, message, details, suggestion}
 */
export function parseApiError(error) {
  let status = 500
  let message = 'An error occurred'
  let details = null
  let suggestion = null

  if (error.status) {
    status = error.status
  }

  if (error.data) {
    // Structured error from API
    details = error.data.detail || error.data.message || null
    message = error.data.error || error.message || message
  } else if (error.message) {
    message = error.message
  }

  // Status-specific messages and suggestions
  switch (status) {
    case 400:
      suggestion = 'Check your input and try again'
      break
    case 409:
      suggestion = 'File has been modified. Refresh and try again'
      break
    case 422:
      suggestion = 'Invalid request format'
      break
    case 500:
      suggestion = 'Server error. Try again later'
      break
    case 503:
      suggestion = 'Server is unavailable. Try again later'
      break
  }

  return { status, message, details, suggestion }
}

/**
 * Error types with specific handlers
 */
export const ErrorTypes = {
  CONFLICT: 'conflict',           // File ETag/version conflict
  COMPILE_ERROR: 'compile_error', // LaTeX compilation error
  PARSE_ERROR: 'parse_error',     // Parse/diff error
  NETWORK_ERROR: 'network_error', // Network failure
  TIMEOUT: 'timeout',             // Request timeout
  VALIDATION: 'validation',       // Input validation
  NOT_FOUND: 'not_found',         // 404
  UNAUTHORIZED: 'unauthorized',   // 401
  FORBIDDEN: 'forbidden',         // 403
  SERVER_ERROR: 'server_error'    // 5xx
}

/**
 * Classify error by type
 * @param {Error} error - Error object
 * @returns {string} ErrorType
 */
export function classifyError(error) {
  if (!error) return ErrorTypes.SERVER_ERROR

  if (error.message?.includes('conflict') || error.status === 409) {
    return ErrorTypes.CONFLICT
  }
  if (error.message?.includes('compile') || error.details?.includes('LaTeX')) {
    return ErrorTypes.COMPILE_ERROR
  }
  if (error.message?.includes('parse') || error.message?.includes('diff')) {
    return ErrorTypes.PARSE_ERROR
  }
  if (error.message?.includes('network') || error.message?.includes('fetch')) {
    return ErrorTypes.NETWORK_ERROR
  }
  if (error.message?.includes('timeout')) {
    return ErrorTypes.TIMEOUT
  }
  if (error.status === 404) return ErrorTypes.NOT_FOUND
  if (error.status === 401) return ErrorTypes.UNAUTHORIZED
  if (error.status === 403) return ErrorTypes.FORBIDDEN
  if (error.status === 422) return ErrorTypes.VALIDATION
  if (error.status >= 500) return ErrorTypes.SERVER_ERROR

  return ErrorTypes.SERVER_ERROR
}

/**
 * Format error for display
 * @param {Error} error - Error object
 * @returns {object} {title, message, details, suggestion, type, action}
 */
export function formatErrorForDisplay(error) {
  const parsed = parseApiError(error)
  const type = classifyError(error)

  let title = 'Error'
  let action = null

  switch (type) {
    case ErrorTypes.CONFLICT:
      title = 'File Conflict'
      action = { label: 'Refresh', callback: 'refresh' }
      break
    case ErrorTypes.COMPILE_ERROR:
      title = 'Compilation Failed'
      action = { label: 'View Errors', callback: 'viewErrors' }
      break
    case ErrorTypes.PARSE_ERROR:
      title = 'Parse Error'
      action = { label: 'View Details', callback: 'viewDetails' }
      break
    case ErrorTypes.NETWORK_ERROR:
      title = 'Connection Lost'
      action = { label: 'Retry', callback: 'retry' }
      break
    case ErrorTypes.TIMEOUT:
      title = 'Request Timeout'
      action = { label: 'Retry', callback: 'retry' }
      break
    case ErrorTypes.NOT_FOUND:
      title = 'Not Found'
      break
    case ErrorTypes.UNAUTHORIZED:
      title = 'Authentication Required'
      action = { label: 'Log In', callback: 'login' }
      break
    case ErrorTypes.FORBIDDEN:
      title = 'Permission Denied'
      break
  }

  return {
    title,
    message: parsed.message,
    details: parsed.details,
    suggestion: parsed.suggestion,
    type,
    action,
    status: parsed.status
  }
}

/**
 * Extract compile errors from API response
 * @param {object} response - Compile response with errors
 * @returns {array} [{line, column, message, file}]
 */
export function parseCompileErrors(response) {
  if (!response?.errors) return []

  return response.errors.map(err => ({
    line: err.line || 0,
    column: err.column || 0,
    message: err.message || 'Unknown error',
    file: err.file || null,
    type: err.type || 'error'
  }))
}

/**
 * Extract diff from conflict error
 * @param {object} error - API error with conflict details
 * @returns {string|null} Unified diff
 */
export function extractConflictDiff(error) {
  if (!error?.data) return null
  return error.data.diff || error.data.conflictDiff || null
}

/**
 * Parse unified diff to extract line-by-line changes
 * @param {string} diff - Unified diff string
 * @returns {Array} Array of hunks with line information
 */
export function parseDiff(diff) {
  if (!diff) return []
  
  const lines = diff.split('\n')
  const hunks = []
  let currentHunk = null
  
  for (const line of lines) {
    // Hunk header: @@ -oldStart,oldCount +newStart,newCount @@
    if (line.startsWith('@@')) {
      const match = line.match(/@@ -(\d+),?(\d*) \+(\d+),?(\d*) @@/)
      if (match) {
        if (currentHunk) hunks.push(currentHunk)
        currentHunk = {
          oldStart: parseInt(match[1]),
          oldCount: parseInt(match[2] || '1'),
          newStart: parseInt(match[3]),
          newCount: parseInt(match[4] || '1'),
          added: [],
          removed: [],
          context: []
        }
      }
    } else if (currentHunk) {
      if (line.startsWith('+') && !line.startsWith('+++')) {
        currentHunk.added.push({ line: line.slice(1), type: 'add' })
      } else if (line.startsWith('-') && !line.startsWith('---')) {
        currentHunk.removed.push({ line: line.slice(1), type: 'remove' })
      } else if (line.startsWith(' ')) {
        currentHunk.context.push({ line: line.slice(1), type: 'context' })
      }
    }
  }
  
  if (currentHunk) hunks.push(currentHunk)
  return hunks
}

/**
 * Format conflict information for display
 * @param {Object} errorData - Error data from 409 response
 * @returns {Object} Formatted conflict info
 */
export function formatConflictInfo(errorData) {
  if (!errorData) return null
  
  const diff = extractConflictDiff({ data: errorData })
  const hunks = diff ? parseDiff(diff) : []
  
  return {
    filePath: errorData.file_path || errorData.path || 'unknown',
    currentEtag: errorData.current_etag,
    expectedEtag: errorData.expected_etag,
    diff,
    hunks,
    hasChanges: hunks.length > 0,
    message: errorData.detail || errorData.message || 'File was modified externally'
  }
}
