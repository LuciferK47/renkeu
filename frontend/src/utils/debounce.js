/**
 * Utility for debouncing function calls.
 * Creates a debounced version of a function that delays execution
 * until after the specified delay has elapsed since the last call.
 * 
 * @param {Function} fn - Function to debounce
 * @param {number} delay - Delay in milliseconds
 * @returns {Function} Debounced function with cancel method
 */
export function debounce(fn, delay = 300) {
  let timeoutId = null
  
  const debounced = function(...args) {
    if (timeoutId) {
      clearTimeout(timeoutId)
    }
    
    timeoutId = setTimeout(() => {
      fn.apply(this, args)
      timeoutId = null
    }, delay)
  }
  
  // Add cancel method to clear pending execution
  debounced.cancel = () => {
    if (timeoutId) {
      clearTimeout(timeoutId)
      timeoutId = null
    }
  }
  
  return debounced
}

/**
 * Throttle function execution to at most once per interval.
 * Different from debounce - guarantees execution at regular intervals.
 * 
 * @param {Function} fn - Function to throttle
 * @param {number} interval - Minimum interval between calls in milliseconds
 * @returns {Function} Throttled function
 */
export function throttle(fn, interval = 300) {
  let lastCall = 0
  let timeoutId = null
  
  return function(...args) {
    const now = Date.now()
    const timeSinceLastCall = now - lastCall
    
    if (timeSinceLastCall >= interval) {
      lastCall = now
      fn.apply(this, args)
    } else {
      // Schedule for next available slot
      if (timeoutId) {
        clearTimeout(timeoutId)
      }
      timeoutId = setTimeout(() => {
        lastCall = Date.now()
        fn.apply(this, args)
      }, interval - timeSinceLastCall)
    }
  }
}

/**
 * Request animation frame based throttle for DOM updates.
 * Ensures function is called at most once per frame for smooth animations.
 * 
 * @param {Function} fn - Function to throttle
 * @returns {Function} RAF-throttled function
 */
export function rafThrottle(fn) {
  let rafId = null
  let lastArgs = null
  
  const throttled = function(...args) {
    lastArgs = args
    
    if (rafId === null) {
      rafId = requestAnimationFrame(() => {
        fn.apply(this, lastArgs)
        rafId = null
        lastArgs = null
      })
    }
  }
  
  throttled.cancel = () => {
    if (rafId !== null) {
      cancelAnimationFrame(rafId)
      rafId = null
      lastArgs = null
    }
  }
  
  return throttled
}
