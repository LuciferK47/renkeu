<script>
  import { onDestroy } from 'svelte'
  import { onMount } from 'svelte'
  
  import { createEventDispatcher } from 'svelte'
  
  const dispatch = createEventDispatcher()
  
  export let pdfPath = ""
  export let loading = false
  export let numPages = 0
  export let currentPage = 1
  export let jobId = null  // Compilation job ID for status polling

  let error = ""
  let compilationStatus = "idle"  // idle, queued, compiling, completed, failed
  let compilationProgress = 0
  let eventSource = null
  let sseActive = false
  let sseReconnectAttempts = 0
  let sseReconnectTimer = null
  let containerRef = null
  let isVisible = false
  let iframeSrc = ''

  // Watch for new compilation jobs
  $: if (jobId) {
    startSse()
  }

  // Update iframe src only when visible and path changes
  $: if (isVisible && pdfPath) {
    iframeSrc = `${pdfPath}#page=${currentPage}`
  }

  onMount(() => {
    // Lazy-load iframe when viewer enters viewport
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
      // Fallback: assume visible
      isVisible = true
    }
  })

  onDestroy(() => {
    stopSse()
    clearReconnectTimer()
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
      eventSource = new EventSource(`/api/compile/stream/${jobId}`)
      sseActive = true

      eventSource.addEventListener('progress', (evt) => {
        const payload = safeParse(evt.data)
        if (!payload) return
        
        // Reset reconnect attempts on successful message
        sseReconnectAttempts = 0
        
        compilationStatus = payload.status || compilationStatus
        compilationProgress = payload.progress ?? compilationProgress

        if (payload.status === 'completed') {
          handleCompletion(payload)
          stopSse()
        } else if (payload.status === 'failed') {
          handleFailure(payload)
          stopSse()
        }
      })

      eventSource.addEventListener('open', () => {
        // Connection opened successfully
        sseReconnectAttempts = 0
      })

      eventSource.onerror = (err) => {
        console.warn('SSE connection error:', err)
        
        // Close the failed connection
        if (eventSource) {
          eventSource.close()
          eventSource = null
        }
        
        // Only reconnect if job is still active (not completed/failed)
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
    
    // Exponential backoff: 1s, 2s, 4s, 8s, max 10s
    const delay = Math.min(1000 * Math.pow(2, sseReconnectAttempts - 1), 10000)
    
    // Give up after 10 attempts (about 1 minute total)
    if (sseReconnectAttempts > 10) {
      error = 'Lost connection to compilation stream. Please refresh.'
      loading = false
      return
    }
    
    clearReconnectTimer()
    sseReconnectTimer = setTimeout(() => {
      console.log(`Reconnecting SSE (attempt ${sseReconnectAttempts})...`)
      connectSse()
    }, delay)
  }

  function handleCompletion(payload) {
    pdfPath = payload.pdf_url
    numPages = payload.diagnostics?.page_count || 0
    loading = false
    error = ""
    dispatch('compiled', {
      pdf_url: payload.pdf_url,
      diagnostics: payload.diagnostics,
      page_count: payload.diagnostics?.page_count || 0
    })
  }

  function handleFailure(payload) {
    error = payload.error || 'Compilation failed'
    loading = false
    dispatch('compileFailed', {
      error: payload.error,
      diagnostics: payload.diagnostics
    })
  }

  onDestroy(() => {
    stopStatusPolling()
    stopSse()
  })

  function handlePageChange(e) {
    const page = parseInt(e.target.value)
    if (page >= 1 && page <= numPages) {
      currentPage = page
    }
  }

  function nextPage() {
    if (currentPage < numPages) {
      currentPage++
    }
  }

  function prevPage() {
    if (currentPage > 1) {
      currentPage--
    }
  }
</script>

<div class="pdf-viewer" bind:this={containerRef}>
  {#if error}
    <div class="error-message">{error}</div>
  {:else if loading}
    <div class="loading">
      <div class="spinner"></div>
      <p>
        {#if compilationStatus === 'queued'}
          Queued for compilation...
        {:else if compilationStatus === 'compiling'}
          Compiling LaTeX... {Math.round(compilationProgress * 100)}%
        {:else}
          Compiling LaTeX...
        {/if}
      </p>
    </div>
  {:else if pdfPath}
    <div class="pdf-controls">
      <button on:click={prevPage} disabled={currentPage === 1}>← Prev</button>
      <input
        type="number"
        min="1"
        max={numPages}
        value={currentPage}
        on:change={handlePageChange}
      />
      <span>of {numPages}</span>
      <button on:click={nextPage} disabled={currentPage === numPages}>Next →</button>
    </div>
    <div class="pdf-frame">
      <iframe
        src={iframeSrc}
        title="PDF Viewer"
      ></iframe>
    </div>
  {:else}
    <div class="no-pdf">No PDF yet. Compile your document first.</div>
  {/if}
</div>

<style>
  .pdf-viewer {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: #fafafa;
  }

  .error-message {
    padding: 1rem;
    background: #ffebee;
    color: #c62828;
    border-bottom: 1px solid #ef5350;
  }

  .loading {
    padding: 2rem;
    text-align: center;
    color: #666;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1rem;
  }

  .spinner {
    width: 40px;
    height: 40px;
    border: 4px solid #f3f3f3;
    border-top: 4px solid #007bff;
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
    color: #999;
  }

  .pdf-controls {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.75rem 1rem;
    background: #fff;
    border-bottom: 1px solid #ddd;
  }

  .pdf-controls button {
    padding: 0.4rem 0.8rem;
    background: #007bff;
    color: white;
    border: none;
    border-radius: 3px;
    cursor: pointer;
    font-size: 12px;
  }

  .pdf-controls button:disabled {
    background: #ccc;
    cursor: not-allowed;
  }

  .pdf-controls button:hover:not(:disabled) {
    background: #0056b3;
  }

  .pdf-controls input {
    width: 60px;
    padding: 0.4rem;
    border: 1px solid #ccc;
    border-radius: 3px;
    text-align: center;
  }

  .pdf-controls span {
    font-size: 13px;
    color: #666;
  }

  .pdf-frame {
    flex: 1;
    overflow: auto;
  }

  .pdf-frame iframe {
    width: 100%;
    height: 100%;
    border: none;
  }
</style>
