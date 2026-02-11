/**
 * Config Service - Handles application configuration
 * - Load/save app config
 * - Manage theme settings
 * - Handle keyboard shortcuts
 * - Persist user preferences
 */

import { api } from '../lib/api/client.js'

export async function loadConfig() {
  return await api.request('GET', '/config')
}

export async function saveConfig(config) {
  return await api.request('POST', '/config', { body: config })
}

export async function updateConfigField(fieldPath, value) {
  const config = { [fieldPath]: value }
  return saveConfig(config)
}

export function getLocalPreferences() {
  return {
    editorHeight: parseFloat(localStorage.getItem('layout:editorHeight') || '60'),
    chatHeight: parseFloat(localStorage.getItem('layout:chatHeight') || '300'),
    recentFiles: JSON.parse(localStorage.getItem('recent_files') || '[]'),
    pinnedFiles: JSON.parse(localStorage.getItem('pinned_files') || '[]'),
    theme: localStorage.getItem('theme:mode') || 'light',
    autoSave: localStorage.getItem('editor:autoSave') !== 'false'
  }
}

export function saveLocalPreference(key, value) {
  localStorage.setItem(key, JSON.stringify(value))
}

export function updateRecentFiles(filePath, maxCount = 20) {
  const recent = JSON.parse(localStorage.getItem('recent_files') || '[]')
  const updated = [filePath, ...recent.filter(f => f !== filePath)].slice(0, maxCount)
  localStorage.setItem('recent_files', JSON.stringify(updated))
  return updated
}

export function togglePinnedFile(filePath) {
  const pinned = JSON.parse(localStorage.getItem('pinned_files') || '[]')
  const updated = pinned.includes(filePath)
    ? pinned.filter(f => f !== filePath)
    : [...pinned, filePath]
  localStorage.setItem('pinned_files', JSON.stringify(updated))
  return updated
}

export async function exportConfig() {
  const config = await loadConfig()
  return JSON.stringify(config, null, 2)
}

export async function importConfig(configJson) {
  try {
    const config = JSON.parse(configJson)
    return await saveConfig(config)
  } catch (e) {
    throw new Error('Invalid config JSON')
  }
}
