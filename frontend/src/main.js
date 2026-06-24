import { mount } from 'svelte'
import App from './App.svelte'

// Crash-visible bootstrap: never show a silent blank screen
function showFatalError(err) {
  const container = document.getElementById('app')
  if (!container) return
  
  const errorEl = document.createElement('div')
  errorEl.style.cssText = `
    padding: 2rem;
    max-width: 800px;
    margin: 2rem auto;
    background: #fee;
    border: 2px solid #c33;
    border-radius: 8px;
    font-family: ui-monospace, 'Cascadia Code', 'Source Code Pro', Menlo, monospace;
  `
  errorEl.innerHTML = `
    <h2 style="margin: 0 0 1rem 0; color: #c33;">[WARNING] Frontend Crashed</h2>
    <pre style="white-space: pre-wrap; word-wrap: break-word; background: #fff; padding: 1rem; border-radius: 4px; overflow-x: auto;">${err?.stack || err?.message || String(err)}</pre>
    <p style="margin: 1rem 0 0 0; color: #666; font-size: 0.9rem;">Check console for details. If this persists, verify backend is running on port 3000.</p>
  `
  container.innerHTML = ''
  container.appendChild(errorEl)
}

try {
  mount(App, {
    target: document.getElementById('app'),
  })

  // Catch unhandled errors after mount
  window.addEventListener('error', (e) => {
    console.error('Unhandled error:', e.error || e.message)
    showFatalError(e.error || e.message)
  })

  window.addEventListener('unhandledrejection', (e) => {
    console.error('Unhandled promise rejection:', e.reason)
    showFatalError(e.reason)
  })
} catch (err) {
  console.error('Mount failed:', err)
  showFatalError(err)
}
