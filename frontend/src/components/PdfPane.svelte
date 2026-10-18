<script>
  // The compiled PDF, all pages in one scrolling column. Pages are PNGs
  // rendered by the server (pdfium) and requested only when they come near
  // the viewport. Every URL names the build, so a page is never stale and the
  // browser may cache it for good.
  import { onMount } from 'svelte'
  import { api } from '../lib/api/client.js'
  import { PaperState } from '../lib/state/paper.svelte.js'
  import { readPref, writePref } from '../lib/storage.js'
  import RegionLayer from './RegionLayer.svelte'

  let {
    compile,
    sourcePath = '',
    onvisualproposal = () => {},
    oncompile = () => {},
    onjump = () => {},
    onlocate = null,
    onnotice = () => {},
    paper = new PaperState()
  } = $props()

  const ZOOMS = [0.5, 0.75, 1, 1.25, 1.5, 2, 3]
  /** Width of a Letter page in CSS px at zoom 1, until a real page says otherwise. */
  const DEFAULT_PAGE_PX = 816

  const savedZoom = readPref('latex-ai:zoom', null)
  let zoom = $state(savedZoom ?? 1)
  /** Until the user picks a zoom, fit the first page to the pane. */
  let autoFit = savedZoom === null
  let currentPage = $state(1)
  let visualFix = $state(false)
  let scroller = $state()
  let basePx = $state(DEFAULT_PAGE_PX)
  let seen = $state(new Set())
  let dark = $state(false)
  /** A region to flash after a forward search: { page, x, y, width, height } in points. */
  let highlight = $state(null)
  let highlightTimer
  /** The region being sent for a visual fix, in points. */
  let selection = $state(null)
  let fixBusy = $state(false)
  let fixError = $state('')
  let instruction = $state('')

  const dpr = Math.min(2, Math.max(1, globalThis.devicePixelRatio || 1))
  const dpi = $derived(Math.min(300, Math.max(48, Math.round(96 * zoom * dpr))))
  const pages = $derived(Array.from({ length: compile.pageCount }, (_, i) => i + 1))
  const file = $derived(compile.mainFile ?? sourcePath)
  // Tone only applies on a dark ground (see PaperState); on light the page
  // already sits on paper-coloured chrome.
  const tone = $derived(dark ? paper.tone : { paper: 1, warmth: 0 })
  const ready = $derived(compile.buildId && compile.pageCount > 0)

  /** Screen px per PDF point at this zoom (CSS px are 1/96 in, points 1/72). */
  const pxPerPt = $derived((zoom * 96) / 72)

  const url = (n) => api.pdfPageUrl(file, n, dpi, tone, compile.buildId)

  onMount(() => {
    const read = () => (dark = document.documentElement.getAttribute('data-theme') === 'dark')
    read()
    const observer = new MutationObserver(read)
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })
    return () => observer.disconnect()
  })

  // Keep the current page in range when the page count shrinks.
  $effect(() => {
    if (currentPage > compile.pageCount) currentPage = Math.max(1, compile.pageCount)
  })

  function markSeen(n) {
    if (!seen.has(n)) seen = new Set(seen).add(n)
  }

  /** Svelte action: load a page when its slot nears the viewport. */
  function watch(node, n) {
    if (typeof IntersectionObserver === 'undefined') {
      markSeen(n)
      return {}
    }
    const io = new IntersectionObserver(
      (entries) => entries.some((e) => e.isIntersecting) && markSeen(n),
      { root: node.parentElement, rootMargin: '800px 0px' }
    )
    io.observe(node)
    return { destroy: () => io.disconnect() }
  }

  function onImageLoad(event) {
    // naturalWidth = pt * dpi/72 and dpi = 96*zoom*dpr, so this is the page's
    // width in CSS px at zoom 1.
    const natural = event.currentTarget.naturalWidth
    if (natural > 0) basePx = natural / dpr / zoom
    if (autoFit && scroller) {
      autoFit = false
      const fit = (scroller.clientWidth - 32) / basePx
      zoom = Math.round(Math.min(1.5, Math.max(0.4, fit)) * 100) / 100
    }
  }

  let ticking = false
  function onScroll() {
    if (ticking || !scroller) return
    ticking = true
    requestAnimationFrame(() => {
      ticking = false
      const probe = scroller.scrollTop + scroller.clientHeight / 3
      let page = 1
      for (const slot of scroller.querySelectorAll('[data-page]')) {
        if (slot.offsetTop <= probe) page = Number(slot.dataset.page)
        else break
      }
      currentPage = page
    })
  }

  function goTo(n) {
    const target = Math.min(Math.max(1, Math.round(n)), compile.pageCount)
    currentPage = target
    scroller?.querySelector(`[data-page="${target}"]`)?.scrollIntoView({ block: 'start' })
  }

  /**
   * Bring a region (PDF points) into view and flash it. Used by forward
   * search. The page is loaded first if it has not been seen yet.
   */
  export function showHit(hit) {
    if (!scroller || !hit) return
    markSeen(hit.page)
    highlight = hit
    clearTimeout(highlightTimer)
    highlightTimer = setTimeout(() => (highlight = null), 3000)
    const slot = scroller.querySelector(`[data-page="${hit.page}"]`)
    if (!slot) return
    const top = slot.offsetTop + hit.y * pxPerPt - scroller.clientHeight / 3
    scroller.scrollTo({ top: Math.max(0, top), behavior: 'smooth' })
  }

  /** Double-click: ask the server which source line is under the pointer. */
  async function jump(event, n) {
    if (visualFix || !compile.buildId) return
    const rect = event.currentTarget.getBoundingClientRect()
    const x = (event.clientX - rect.left) / pxPerPt
    const y = (event.clientY - rect.top) / pxPerPt
    try {
      const where = await api.synctexBackward({ build: compile.buildId, page: n, x, y })
      onjump(where)
    } catch (e) {
      onnotice(e.status === 404 ? 'No source position for that spot.' : `Could not find the source: ${e.message}`)
    }
  }

  async function requestFix(region) {
    selection = region
    fixBusy = true
    fixError = ''
    try {
      const { page, x, y, w, h } = region
      const result = await api.repairVisualCrop({
        filePath: file,
        buildId: compile.buildId,
        page,
        x,
        y,
        w,
        h,
        instruction: instruction.trim() || undefined
      })
      // Review-first: hand the proposal up, never apply it here.
      onvisualproposal(result)
      visualFix = false
      instruction = ''
    } catch (e) {
      fixError = e.message || 'Visual fix failed'
    } finally {
      fixBusy = false
      selection = null
    }
  }

  // Ctrl+wheel (and trackpad pinch, which browsers report the same way) zooms
  // the page instead of scrolling it. Needs a non-passive listener.
  function zoomOnWheel(node) {
    const handler = (e) => {
      if (!e.ctrlKey) return
      e.preventDefault()
      setZoom(zoom * (e.deltaY < 0 ? 1.1 : 1 / 1.1))
    }
    node.addEventListener('wheel', handler, { passive: false })
    return { destroy: () => node.removeEventListener('wheel', handler) }
  }

  function setZoom(z) {
    autoFit = false
    zoom = Math.min(3, Math.max(0.25, z))
    writePref('latex-ai:zoom', zoom)
  }

  const step = (dir) => {
    const next = dir > 0 ? ZOOMS.find((z) => z > zoom + 0.01) : [...ZOOMS].reverse().find((z) => z < zoom - 0.01)
    if (next) setZoom(next)
  }

  function fitWidth() {
    if (scroller) setZoom((scroller.clientWidth - 32) / basePx)
  }
</script>

<section class="pdf-pane" aria-label="PDF preview">
  <div class="controls">
    <button onclick={() => goTo(currentPage - 1)} disabled={!ready || currentPage <= 1} aria-label="Previous page">‹</button>
    <label class="page">
      <input
        type="number"
        min="1"
        max={compile.pageCount || 1}
        value={currentPage}
        disabled={!ready}
        onchange={(e) => goTo(Number(e.currentTarget.value))}
        aria-label="Page number"
      />
      <span>/ {compile.pageCount || '–'}</span>
    </label>
    <button onclick={() => goTo(currentPage + 1)} disabled={!ready || currentPage >= compile.pageCount} aria-label="Next page">›</button>

    <span class="sep"></span>
    <button onclick={() => step(-1)} disabled={!ready} aria-label="Zoom out">−</button>
    <span class="zoom" aria-live="polite">{Math.round(zoom * 100)}%</span>
    <button onclick={() => step(1)} disabled={!ready} aria-label="Zoom in">+</button>
    <button onclick={fitWidth} disabled={!ready}>Fit width</button>

    <span class="spacer"></span>
    {#if onlocate}
      <button onclick={onlocate} disabled={!ready} title="Show the line the cursor is on (Ctrl+Alt+J)">Find line</button>
    {/if}
    {#if dark && ready}
      <button
        class:on={paper.revealing}
        onpointerdown={paper.startReveal}
        onpointerup={paper.endReveal}
        onpointerleave={paper.endReveal}
        onblur={paper.endReveal}
        onkeydown={(e) => (e.key === ' ' || e.key === 'Enter') && paper.startReveal()}
        onkeyup={paper.endReveal}
        title="Hold to see the page in its true colours"
        aria-pressed={paper.revealing}
      >True colours</button>
    {/if}
    <button class:on={visualFix} onclick={() => (visualFix = !visualFix)} disabled={!ready} title="Draw a box over the page to ask for a fix">
      {visualFix ? 'Close visual fix' : 'Visual fix'}
    </button>
    {#if compile.buildId && compile.pageCount > 0}
      <a class="export" href={api.exportPdfUrl(compile.buildId)} download>Export PDF</a>
    {/if}
  </div>

  {#if compile.compiling}
    <div class="bar" role="progressbar" aria-valuenow={compile.progress} aria-valuemin="0" aria-valuemax="100">
      <div class="fill" style:width={`${Math.max(8, compile.progress)}%`}></div>
    </div>
  {/if}

  {#if visualFix && ready}
    <div class="fixbar">
      <input
        type="text"
        placeholder="What should change? (optional) — then drag a box over the problem"
        bind:value={instruction}
        disabled={fixBusy}
        aria-label="What should change"
      />
      {#if fixBusy}<span role="status">Asking the AI…</span>{/if}
      {#if fixError}<span class="bad" role="alert">{fixError}</span>{/if}
    </div>
  {/if}

  {#if ready}
    <div class="scroller" bind:this={scroller} onscroll={onScroll} use:zoomOnWheel>
      {#each pages as n (n)}
        <div
          class="slot"
          data-page={n}
          use:watch={n}
          style:width={`${basePx * zoom}px`}
          style:min-height={`${basePx * zoom * 1.294}px`}
          role="presentation"
          ondblclick={(e) => jump(e, n)}
          title={visualFix ? undefined : 'Double-click to jump to the source'}
        >
          {#if seen.has(n)}
            <img src={url(n)} alt={`Page ${n}`} draggable="false" onload={onImageLoad} style:width="100%" />
          {:else}
            <div class="placeholder" style:aspect-ratio="0.7727">{n}</div>
          {/if}
          {#if highlight && highlight.page === n}
            <div
              class="hit"
              style:left={`${highlight.x * pxPerPt}px`}
              style:top={`${highlight.y * pxPerPt}px`}
              style:width={`${highlight.width * pxPerPt}px`}
              style:height={`${highlight.height * pxPerPt}px`}
            ></div>
          {/if}
          {#if visualFix}
            <RegionLayer page={n} {pxPerPt} {selection} disabled={fixBusy} onselect={requestFix} />
          {/if}
        </div>
      {/each}
    </div>
  {:else}
    <div class="empty">
      {#if compile.compiling}
        <p>Compiling…</p>
      {:else if compile.status === 'error'}
        <p>The last compile failed, so there is no PDF.</p>
        <p class="hint">See Problems below.</p>
      {:else}
        <p>No PDF yet</p>
        <button class="primary" onclick={oncompile} disabled={!sourcePath}>Compile</button>
      {/if}
    </div>
  {/if}
</section>

<style>
  .pdf-pane { display: flex; flex-direction: column; height: 100%; min-width: 0; background: hsl(var(--surface-2)); }
  .controls { display: flex; align-items: center; gap: 0.35rem; padding: 0.3rem 0.5rem; border-bottom: 1px solid hsl(var(--border)); background: hsl(var(--surface-1)); flex-wrap: wrap; font-size: 12px; }
  .controls button, .export { padding: 0.15rem 0.5rem; background: none; border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); color: hsl(var(--foreground)); font: inherit; cursor: pointer; text-decoration: none; }
  .controls button:disabled { opacity: 0.4; cursor: default; }
  .controls button.on { background: hsl(var(--accent)); }
  .page { display: flex; align-items: center; gap: 0.25rem; }
  .page input { width: 3.2rem; padding: 0.1rem 0.25rem; background: hsl(var(--surface-0)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); color: inherit; font: inherit; }
  .zoom { min-width: 2.8rem; text-align: center; }
  .sep { width: 1px; align-self: stretch; background: hsl(var(--border)); margin: 0 0.25rem; }
  .spacer { flex: 1; }
  .bar { height: 2px; background: hsl(var(--border)); }
  .fill { height: 100%; background: hsl(var(--primary)); transition: width 0.2s; }
  .scroller { flex: 1; overflow: auto; padding: 1rem; display: flex; flex-direction: column; align-items: center; gap: 1rem; }
  .slot { position: relative; flex-shrink: 0; background: hsl(var(--paper)); box-shadow: 0 1px 4px rgb(0 0 0 / 0.25); }
  .slot img { display: block; }
  .placeholder { display: grid; place-content: center; color: hsl(var(--text-faint)); }
  .fixbar { display: flex; align-items: center; gap: 0.5rem; padding: 0.3rem 0.5rem; border-bottom: 1px solid hsl(var(--border)); background: hsl(var(--surface-1)); font-size: 12px; }
  .fixbar input { flex: 1; min-width: 0; padding: 0.2rem 0.4rem; background: hsl(var(--surface-0)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); color: inherit; font: inherit; }
  .bad { color: hsl(var(--err)); }
  .hit { position: absolute; pointer-events: none; border-radius: 2px; background: hsl(var(--primary) / 0.28); outline: 2px solid hsl(var(--primary)); animation: flash 3s ease-out forwards; }
  @keyframes flash { 0%, 70% { opacity: 1; } 100% { opacity: 0; } }
  .empty { flex: 1; display: grid; place-content: center; text-align: center; color: hsl(var(--text-dim)); gap: 0.5rem; }
  .hint { font-size: 12px; color: hsl(var(--text-faint)); }
  .primary { padding: 0.35rem 1rem; background: hsl(var(--primary)); color: hsl(var(--primary-foreground)); border: none; border-radius: var(--radius-xs); cursor: pointer; }
  .primary:disabled { opacity: 0.4; }
</style>
