// File store: manages current file state (path, content, etag, dirty)
import { writable } from 'svelte/store'

export const fileStore = writable({
  filePath: null,
  content: '',
  etag: null,
  dirty: false,
  lastSavedAt: null,
  savedContent: ''
})

export function setFile(path, content, etag) {
  fileStore.set({
    filePath: path,
    content,
    etag,
    dirty: false,
    lastSavedAt: new Date(),
    savedContent: content
  })
}

export function updateContent(newContent) {
  fileStore.update(f => ({
    ...f,
    content: newContent,
    dirty: newContent !== f.savedContent
  }))
}

export function markDirty() {
  fileStore.update(f => ({...f, dirty: true}))
}

export function markSaved(newContent, newEtag) {
  fileStore.update(f => ({
    ...f,
    content: newContent,
    savedContent: newContent,
    dirty: false,
    etag: newEtag,
    lastSavedAt: new Date()
  }))
}
