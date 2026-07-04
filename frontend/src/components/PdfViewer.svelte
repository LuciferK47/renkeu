<script>
  import { onDestroy, onMount, createEventDispatcher } from 'svelte'
  import { api } from '../lib/api/client.js'
  
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

  let visualFixMode = false;
  let visualFixing = false;
  let isDrawing = false;
  let startX = 0, startY = 0;
  let box = null;

  $: if (jobId) {
    startSse()
  }

  $: pageImgUrl = (isVisible && pdfPath && filePath) 
    ? api.buildUrl(`/pdf/page?file=${encodeURIComponent(filePath)}&n=${currentPage}`) 
    : '';

  onMount(() => {
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
      eventSource = new EventSource(api.buildUrl(`/compile/stream/${jobId}`))
      sseActive = true

      eventSource.addEventListener('progress', (evt) => {
        const payload = JSON.parse(evt.data)
        if (!payload) return
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
    pdfPath = payload.pdf_url
    numPages = payload.diagnostics?.page_count || 0
    loading = false
    error = ""
    // We increment page here simply to trigger reload if it was already on page 1, but svelte handles reactivity
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

  function handleMouseDown(e) {
    if (!visualFixMode || visualFixing) return;
    const rect = imgContainer.getBoundingClientRect();
    startX = e.clientX - rect.left;
    startY = e.clientY - rect.top;
    isDrawing = true;
    box = { x: startX, y: startY, w: 0, h: 0 };
  }

  function handleMouseMove(e) {
    if (!isDrawing) return;
    const rect = imgContainer.getBoundingClientRect();
    const currX = Math.max(0, Math.min(e.clientX - rect.left, rect.width));
    const currY = Math.max(0, Math.min(e.clientY - rect.top, rect.height));
    box = {
      x: Math.min(startX, currX),
      y: Math.min(startY, currY),
      w: Math.abs(currX - startX),
      h: Math.abs(currY - startY)
    };
  }

  async function handleMouseUp(e) {
    if (!isDrawing) return;
    isDrawing = false;
    if (box && box.w > 10 && box.h > 10) {
      visualFixing = true;
      const rect = imgContainer.getBoundingClientRect();
      const imgElement = imgContainer.querySelector('img');
      const scaleX = imgElement.naturalWidth / rect.width;
      const scaleY = imgElement.naturalHeight / rect.height;
      
      const apiBox = {
        x: Math.round(box.x * scaleX),
        y: Math.round(box.y * scaleY),
        w: Math.round(box.w * scaleX),
        h: Math.round(box.h * scaleY),
      };
      
      try {
         const res = await api.repairVisualCrop(filePath, currentPage, apiBox.x, apiBox.y, apiBox.w, apiBox.h);
         if (res && res.edits && res.edits.length > 0) {
            dispatch('apply-selected', res.edits);
         }
      } catch (err) {
         error = err.message;
      } finally {
         visualFixing = false;
         box = null;
         visualFixMode = false;
      }
    } else {
      box = null;
    }
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
      <button 
        class="visual-fix-btn" 
        class:active={visualFixMode}
        on:click={() => { visualFixMode = !visualFixMode; box = null; }}
        title="Draw a bounding box to fix an issue"
      >
        {visualFixMode ? 'Cancel Visual Fix' : 'Visual Fix'}
      </button>
    </div>
    
    <div class="pdf-frame">
      <!-- svelte-ignore a11y-no-static-element-interactions -->
      <div 
        class="img-container" 
        bind:this={imgContainer}
        on:mousedown={handleMouseDown}
        on:mousemove={handleMouseMove}
        on:mouseup={handleMouseUp}
        on:mouseleave={handleMouseUp}
        style="cursor: {visualFixMode ? 'crosshair' : 'default'}"
      >
        <!-- svelte-ignore a11y-missing-attribute -->
        <img src={pageImgUrl} class="pdf-img" on:load={onImageLoad} draggable="false" />
        {#if box}
          <div class="bbox" style="left: {box.x}px; top: {box.y}px; width: {box.w}px; height: {box.h}px;"></div>
        {/if}
        {#if visualFixing}
          <div class="visual-overlay fixing">
            <div class="overlay-msg">Analyzing visual selection...</div>
          </div>
        {/if}
      </div>
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
    background: #fafafa;
    position: relative;
  }
  
  .spacer { flex: 1; }
  
  .visual-fix-btn {
    background: #4caf50 !important;
  }
  .visual-fix-btn.active {
    background: #e53935 !important;
  }
  
  .pdf-frame {
    flex: 1;
    overflow: auto;
    position: relative;
    display: flex;
    justify-content: center;
    background: #e0e0e0;
    padding: 20px;
  }

  .img-container {
    position: relative;
    box-shadow: 0 4px 12px rgba(0,0,0,0.1);
    background: white;
    display: inline-block;
  }

  .pdf-img {
    display: block;
    max-width: 100%;
    height: auto;
  }

  .bbox {
    position: absolute;
    border: 2px solid #e53935;
    background: rgba(229, 57, 53, 0.2);
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
    background: white;
    padding: 10px 20px;
    border-radius: 4px;
    font-weight: bold;
    box-shadow: 0 2px 8px rgba(0,0,0,0.2);
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
</style>
