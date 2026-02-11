/**
 * Event bus for application-wide events
 * Enables event-driven architecture and loose coupling between components
 */

import { writable } from 'svelte/store'

// Event types
export const EventType = {
  // File operations
  FILE_OPENED: 'file:opened',
  FILE_SAVED: 'file:saved',
  FILE_CLOSED: 'file:closed',
  FILE_CHANGED: 'file:changed',
  
  // Edit operations
  EDIT_APPLIED: 'edit:applied',
  EDIT_UNDONE: 'edit:undone',
  EDIT_REDONE: 'edit:redone',
  
  // Compilation
  COMPILE_STARTED: 'compile:started',
  COMPILE_COMPLETED: 'compile:completed',
  COMPILE_FAILED: 'compile:failed',
  COMPILE_CANCELLED: 'compile:cancelled',
  
  // History
  HISTORY_ADDED: 'history:added',
  HISTORY_CLEARED: 'history:cleared',
  
  // AI Planning
  AI_PLAN_STARTED: 'ai:plan_started',
  AI_PLAN_COMPLETED: 'ai:plan_completed',
  AI_PLAN_FAILED: 'ai:plan_failed',
  
  // Errors
  ERROR_OCCURRED: 'error:occurred',
  WARNING_OCCURRED: 'warning:occurred'
}

class EventBus {
  constructor() {
    this.listeners = new Map()
    this.eventHistory = []
    this.maxHistorySize = 100
  }

  /**
   * Subscribe to an event
   * @param {string} eventType - Event type to listen for
   * @param {Function} handler - Callback function
   * @returns {Function} Unsubscribe function
   */
  on(eventType, handler) {
    if (!this.listeners.has(eventType)) {
      this.listeners.set(eventType, new Set())
    }
    
    this.listeners.get(eventType).add(handler)
    
    // Return unsubscribe function
    return () => this.off(eventType, handler)
  }

  /**
   * Unsubscribe from an event
   * @param {string} eventType - Event type
   * @param {Function} handler - Handler to remove
   */
  off(eventType, handler) {
    const handlers = this.listeners.get(eventType)
    if (handlers) {
      handlers.delete(handler)
      if (handlers.size === 0) {
        this.listeners.delete(eventType)
      }
    }
  }

  /**
   * Emit an event
   * @param {string} eventType - Event type
   * @param {*} data - Event data
   */
  emit(eventType, data = null) {
    const event = {
      type: eventType,
      data,
      timestamp: new Date().toISOString()
    }
    
    // Add to history
    this.eventHistory.push(event)
    if (this.eventHistory.length > this.maxHistorySize) {
      this.eventHistory.shift()
    }
    
    // Notify listeners
    const handlers = this.listeners.get(eventType)
    if (handlers) {
      handlers.forEach(handler => {
        try {
          handler(data, event)
        } catch (error) {
          console.error(`Error in event handler for ${eventType}:`, error)
        }
      })
    }
    
    // Also notify wildcard listeners
    const wildcardHandlers = this.listeners.get('*')
    if (wildcardHandlers) {
      wildcardHandlers.forEach(handler => {
        try {
          handler(event)
        } catch (error) {
          console.error('Error in wildcard event handler:', error)
        }
      })
    }
  }

  /**
   * Subscribe to all events
   * @param {Function} handler - Callback function receiving all events
   * @returns {Function} Unsubscribe function
   */
  onAny(handler) {
    return this.on('*', handler)
  }

  /**
   * Get recent event history
   * @param {number} limit - Max events to return
   * @returns {Array} Recent events
   */
  getHistory(limit = 50) {
    return this.eventHistory.slice(-limit)
  }

  /**
   * Clear all listeners
   */
  clear() {
    this.listeners.clear()
  }

  /**
   * Get all active event types
   * @returns {Array<string>} Event types with listeners
   */
  getActiveEventTypes() {
    return Array.from(this.listeners.keys()).filter(type => type !== '*')
  }
}

// Global event bus instance
export const eventBus = new EventBus()

// Svelte store for reactive event history
export const recentEvents = writable([])

// Update store when events occur
eventBus.onAny((event) => {
  recentEvents.update(events => {
    const updated = [...events, event].slice(-20) // Keep last 20 events
    return updated
  })
})

export default eventBus
