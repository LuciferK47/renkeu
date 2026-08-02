<script>
  import { onDestroy, onMount, createEventDispatcher } from 'svelte'
  import { api } from '../lib/api/client.js'
  import { paperStore, activeTone } from '../stores/paperStore.js'
  import VisualFixOverlay from './VisualFixOverlay.svelte'
  
  const dispatch = createEventDispatcher()
  
  export let pdfPath = ""
  export let filePath = ""
  export let loading = false
  export let numPages = 0
  export let currentPage = 1
  export let jobId = null

  let error = ""
  let compilationStatus = "idle"
  let compilationProgress = 0
  let eventSource = null
  let sseActive = false
  let sseReconnectAttempts = 0
  let sseReconnectTimer = null
  let containerRef = null
  let isVisible = false
  let imgContainer = null

  // Region selection lives in VisualFixOverlay (masterplan §12 M3).
  let visualFixMode = false;

  $: if (jobId) {
    startSse()
  }

  // Tone only applies on a dark ground: on a light theme the page already sits
  // on paper-coloured chrome, and dimming it there would be a loss for nothing.
  $: tone = isDark ? $activeTone : { paper: 1, warmth: 0 };

  $: pageImgUrl = (isVisible && filePath && numPages > 0)
    ? api.pdfPageUrl(filePath, currentPage, undefined, tone)
    : '';

  // Watch the theme attribute rather than a media query: the in-app toggle is
  // what decides, and it can disagree with the OS.
  let isDark = false
  let themeObserver = null

  onMount(() => {
    const readTheme = () => {
      isDark = document.documentElement.getAttribute('data-theme') === 'dark'
    }
    readTheme()
    themeObserver = new MutationObserver(readTheme)
    themeObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme'],
    })

    if ('IntersectionObserver' in window) {
      const observer = new IntersectionObserver((entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            isVisible = true
            observer.disconnect()
            break
          }
        }
      }, { root: null, threshold: 0.1 })
      if (containerRef) observer.observe(containerRef)
    } else {
      isVisible = true
    }
  })

  onDestroy(() => {
    stopSse()
    clearReconnectTimer()
    themeObserver?.disconnect()
  })

  function clearReconnectTimer() {
    if (sseReconnectTimer) {
      clearTimeout(sseReconnectTimer)
      sseReconnectTimer = null
    }
  }

  function stopSse() {
    if (eventSource) {
      eventSource.close()
      eventSource = null
    }
    sseActive = false
    clearReconnectTimer()
  }

  function startSse() {
    stopSse()
    compilationStatus = "queued"
    loading = true
    error = ""
    sseReconnectAttempts = 0
    connectSse()
  }

  function connectSse() {
    if (!jobId) return

    try {
      eventSource = new EventSource(api.compileStreamUrl(jobId))
      sseActive = true

      // Locked §8.2 taxonomy: progress* -> compiled -> done
      eventSource.addEventListener('progress', (evt) => {
        const payload = JSON.parse(evt.data)
        if (!payload) return
        sseReconnectAttempts = 0
        compilationStatus = 'compiling'
        compilationProgress = (payload.progress ?? 0) * 100
      })

      eventSource.addEventListener('compiled', (evt) => {
        const payload = JSON.parse(evt.data)
        if (payload.success) {
          handleCompletion(payload)
        } else {
          handleFailure(payload)
        }
        stopSse()
      })

      eventSource.addEventListener('open', () => { sseReconnectAttempts = 0 })

      eventSource.onerror = (err) => {
        console.warn('SSE connection error:', err)
        if (eventSource) { eventSource.close(); eventSource = null; }
        if (compilationStatus === 'queued' || compilationStatus === 'compiling') {
          reconnectSse()
        }
      }
    } catch (e) {
      console.error('Failed to create SSE connection:', e)
      error = 'Failed to connect to compilation stream'
      loading = false
    }
  }

  function reconnectSse() {
    sseReconnectAttempts++
    const delay = Math.min(1000 * Math.pow(2, sseReconnectAttempts - 1), 10000)
    if (sseReconnectAttempts > 10) {
      error = 'Lost connection to compilation stream. Please refresh.'
      loading = false
      return
    }
    clearReconnectTimer()
    sseReconnectTimer = setTimeout(() => { connectSse() }, delay)
  }

  function handleCompletion(payload) {
    numPages = payload.page_count || 1
    currentPage = Math.min(currentPage || 1, numPages)
    compilationStatus = 'completed'
    loading = false
    error = ""
    dispatch('compiled', { page_count: numPages })
  }

  function handleFailure(payload) {
    error = payload.error_summary || 'Compilation failed'
    compilationStatus = 'failed'
    loading = false
    dispatch('compileFailed', { error })
  }

  function onImageLoad() {
    // Force a re-layout if needed
  }
</script>

<div class="pdf-viewer" bind:this={containerRef}>
  {#if error}
    <div class="error-message">{error}</div>
  {/if}

  {#if pdfPath && filePath}
    <div class="pdf-controls">
      <button on:click={() => currentPage = Math.max(1, currentPage - 1)} disabled={currentPage <= 1}>Previous</button>
      <input type="number" bind:value={currentPage} min="1" max={numPages || 100} />
      <span>of {numPages || '?'}</span>
      <button on:click={() => currentPage = Math.min(numPages || 100, currentPage + 1)} disabled={numPages ? currentPage >= numPages : false}>Next</button>
      
      <div class="spacer"></div>
      {#if isDark}
        <!-- Press and hold to see the page exactly as it will print. Pointer
             and keyboard both, because checking a figure before submitting is
             not a mouse-only task. -->
        <button
          class="reveal-btn"
          class:revealing={$paperStore.revealing}
          on:pointerdown={paperStore.startReveal}
          on:pointerup={paperStore.endReveal}
          on:pointerleave={paperStore.endReveal}
          on:keydown={(e) => { if (e.key === ' ' || e.key === 'Enter') paperStore.startReveal() }}
          on:keyup={paperStore.endReveal}
          on:blur={paperStore.endReveal}
          title="Hold to see the page in its true colours"
          aria-pressed={$paperStore.revealing}
        >
          {$paperStore.revealing ? 'True colours' : 'Hold for true colours'}
        </button>
      {/if}
      <button
        class="visual-fix-btn"
        class:active={visualFixMode}
        on:click={() => (visualFixMode = !visualFixMode)}
        title="Draw a box over the page to ask for a fix"
      >
        {visualFixMode ? 'Close Visual Fix' : 'Visual Fix'}
      </button>
    </div>
    
    <div class="pdf-frame">
      {#if visualFixMode}
        <!-- Region selection + proposal review (masterplan §12 M3). -->
        <VisualFixOverlay
          {filePath}
          page={currentPage}
          active={true}
          on:proposal={(e) => dispatch('visual-proposal', e.detail)}
        />
      {:else}
        <div class="img-container" bind:this={imgContainer}>
          <img src={pageImgUrl} class="pdf-img" alt={`Page ${currentPage}`} on:load={onImageLoad} draggable="false" />
        </div>
      {/if}
    </div>
  {:else if loading}
    <div class="loading">
      <div class="spinner"></div>
      <div>
        {#if compilationStatus === 'queued'}
          Queued for compilation...
        {:else if compilationStatus === 'compiling'}
          Compiling LaTeX... {Math.round(compilationProgress * 100)}%
        {:else}
          Loading PDF...
        {/if}
      </div>
    </div>
  {:else}
    <div class="no-pdf">No PDF compiled yet.</div>
  {/if}
</div>

<style>
  .pdf-viewer {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: hsl(var(--surface-2));
    position: relative;
  }
  
  .spacer { flex: 1; }
  
  .visual-fix-btn {
    background: hsl(var(--ok)) !important;
  }
  .visual-fix-btn.active {
    background: hsl(var(--err)) !important;
  }
  
  .pdf-frame {
    flex: 1;
    overflow: auto;
    position: relative;
    display: flex;
    justify-content: center;
    background: hsl(var(--muted));
    padding: 20px;
  }

  .reveal-btn {
    padding: 4px 12px;
    font-size: 12px;
    border: 1px solid hsl(var(--border));
    border-radius: var(--radius-pill);
    background: transparent;
    color: hsl(var(--text-dim));
    cursor: pointer;
  }

  .reveal-btn:hover,
  .reveal-btn.revealing {
    background: hsl(var(--accent));
    color: hsl(var(--foreground));
  }

  .img-container {
    position: relative;
    box-shadow: 0 4px 12px rgba(0,0,0,0.1);
    background: hsl(var(--paper));
    display: inline-block;
  }

  .pdf-img {
    display: block;
    max-width: 100%;
    height: auto;
  }

  .bbox {
    position: absolute;
    border: 2px solid hsl(var(--err));
    background: hsl(var(--err) / 0.2);
    pointer-events: none;
  }

  .visual-overlay {
    position: absolute;
    top: 0; left: 0; width: 100%; height: 100%;
    background: rgba(0,0,0,0.4);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10;
  }
  
  .overlay-msg {
    background: hsl(var(--surface-1));
    padding: 10px 20px;
    border-radius: var(--radius-xs);
    font-weight: bold;
    box-shadow: 0 2px 8px rgba(0,0,0,0.2);
  }

  .error-message {
    padding: 1rem;
    background: hsl(var(--err) / 0.1);
    color: hsl(var(--err));
    border-bottom: 1px solid hsl(var(--err));
  }

  .loading {
    padding: 2rem;
    text-align: center;
    color: hsl(var(--text-dim));
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1rem;
  }

  .spinner {
    width: 40px;
    height: 40px;
    /* A rotating arc, not a side accent: the thick ring is the spinner. */
    border: 4px solid hsl(var(--line-soft));
    border-top: 4px solid hsl(var(--primary));
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    0% { transform: rotate(0deg); }
    100% { transform: rotate(360deg); }
  }

  .no-pdf {
    padding: 2rem;
    text-align: center;
    color: hsl(var(--text-faint));
  }

  .pdf-controls {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.75rem 1rem;
    background: hsl(var(--surface-1));
    border-bottom: 1px solid hsl(var(--border));
  }

  .pdf-controls button {
    padding: 0.4rem 0.8rem;
    background: hsl(var(--primary));
    color: hsl(var(--primary-foreground));
    border: none;
    border-radius: var(--radius-xs);
    cursor: pointer;
    font-size: 12px;
  }

  .pdf-controls button:disabled {
    background: hsl(var(--accent));
    cursor: not-allowed;
  }

  .pdf-controls button:hover:not(:disabled) {
    background: hsl(var(--primary));
  }

  .pdf-controls input {
    width: 60px;
    padding: 0.4rem;
    border: 1px solid hsl(var(--border));
    border-radius: var(--radius-xs);
    text-align: center;
  }

  .pdf-controls span {
    font-size: 13px;
    color: hsl(var(--text-dim));
  }
</style>
