import { mount } from 'svelte'
import App from './App.svelte'

// A failed mount means there is no UI at all, so a visible message beats a
// blank page. Errors AFTER mount must not do this: replacing #app throws away
// the user's unsaved buffer, and benign browser noise (a ResizeObserver loop
// warning, one rejected fetch) would trigger it.
function showFatalError(err) {
  const container = document.getElementById('app')
  if (!container) return

  const box = document.createElement('div')
  box.style.cssText =
    "padding:2rem;max-width:800px;margin:2rem auto;background:#fee;border:2px solid #c33;border-radius:8px;font-family:ui-monospace,Menlo,monospace;"

  const title = document.createElement('h2')
  title.style.cssText = 'margin:0 0 1rem 0;color:#c33;'
  title.textContent = 'Renkeu failed to start'

  // textContent, never innerHTML: the message can contain markup.
  const detail = document.createElement('pre')
  detail.style.cssText =
    'white-space:pre-wrap;word-wrap:break-word;background:#fff;padding:1rem;border-radius:4px;overflow-x:auto;'
  detail.textContent = err?.stack || err?.message || String(err)

  const hint = document.createElement('p')
  hint.style.cssText = 'margin:1rem 0 0 0;color:#666;font-size:0.9rem;'
  hint.textContent = 'See the console for details, and check that the backend is running.'

  box.append(title, detail, hint)
  container.replaceChildren(box)
}

try {
  mount(App, { target: document.getElementById('app') })
} catch (err) {
  console.error('Mount failed:', err)
  showFatalError(err)
}

window.addEventListener('error', (e) => {
  console.error('Unhandled error:', e.error || e.message)
})
window.addEventListener('unhandledrejection', (e) => {
  console.error('Unhandled promise rejection:', e.reason)
})
