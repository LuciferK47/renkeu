<script>
  /**
   * Visual grounding (masterplan §12 M3): drag a box over the rendered page,
   * the backend maps it to the source line via SyncTeX and returns a proposal.
   *
   * The proposal is surfaced for review — this component never applies edits.
   */
  import { createEventDispatcher } from 'svelte';
  import { api } from '../lib/api/client.js';
  import { normalizeDrag, isMeaningfulDrag, toImagePixels } from '../utils/bbox.js';

  const dispatch = createEventDispatcher();

  export let filePath = '';
  export let page = 1;
  /** Must match the DPI the page image was rendered at. */
  export let dpi = 150;
  /** Display zoom; 1 = natural size of the rendered PNG. */
  export let zoom = 1;
  export let active = false;

  let imgEl = null;
  let dragging = false;
  let start = null;
  let box = null;
  let busy = false;
  let error = '';
  let instruction = '';
  let result = null;

  $: pageUrl = filePath ? api.pdfPageUrl(filePath, page, dpi) : '';
  // Changing page/file invalidates any in-progress selection.
  $: if (page || filePath) resetSelection();

  function resetSelection() {
    box = null;
    dragging = false;
    start = null;
  }

  function localPoint(event) {
    const rect = imgEl.getBoundingClientRect();
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  }

  function bounds() {
    const rect = imgEl.getBoundingClientRect();
    return { width: rect.width, height: rect.height };
  }

  function onPointerDown(event) {
    if (!active || busy || !imgEl) return;
    imgEl.setPointerCapture?.(event.pointerId);
    start = localPoint(event);
    box = { ...start, w: 0, h: 0 };
    dragging = true;
    error = '';
  }

  function onPointerMove(event) {
    if (!dragging) return;
    box = normalizeDrag(start, localPoint(event), bounds());
  }

  async function onPointerUp(event) {
    if (!dragging) return;
    dragging = false;
    imgEl?.releasePointerCapture?.(event.pointerId);

    if (!isMeaningfulDrag(box)) {
      box = null;
      return;
    }
    await requestFix();
  }

  async function requestFix() {
    if (!box || !imgEl) return;

    const region = toImagePixels(
      box,
      { width: imgEl.clientWidth, height: imgEl.clientHeight },
      { width: imgEl.naturalWidth, height: imgEl.naturalHeight }
    );

    busy = true;
    error = '';
    try {
      result = await api.repairVisualCrop({
        filePath,
        page,
        dpi,
        instruction: instruction.trim() || undefined,
        ...region
      });
      // Review-first: hand the proposal up, never apply it here.
      dispatch('proposal', result);
    } catch (err) {
      error = err.message || 'Visual fix failed';
      result = null;
    } finally {
      busy = false;
    }
  }

  function dismiss() {
    result = null;
    box = null;
  }
</script>

<div class="overlay-root">
  <div class="toolbar">
    <label class="toggle">
      <input type="checkbox" bind:checked={active} />
      Select a region
    </label>
    <input
      class="instruction"
      type="text"
      placeholder="What should change here? (optional)"
      bind:value={instruction}
      disabled={busy}
    />
    {#if busy}<span class="status" role="status">Analysing region…</span>{/if}
  </div>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  <div class="page-wrap" class:selecting={active}>
    <!-- svelte-ignore a11y-no-noninteractive-element-interactions -->
    <img
      bind:this={imgEl}
      src={pageUrl}
      alt={`Page ${page} of ${filePath}`}
      style:width={`${zoom * 100}%`}
      draggable="false"
      on:pointerdown={onPointerDown}
      on:pointermove={onPointerMove}
      on:pointerup={onPointerUp}
      on:pointercancel={resetSelection}
    />
    {#if box}
      <div
        class="selection"
        style:left={`${box.x}px`}
        style:top={`${box.y}px`}
        style:width={`${box.w}px`}
        style:height={`${box.h}px`}
      ></div>
    {/if}
  </div>

  {#if result}
    <aside class="proposal">
      <header>
        <strong>{result.edits.length} proposed edit{result.edits.length === 1 ? '' : 's'}</strong>
        <span class="hit">
          {result.file}:{result.line}{result.exact ? '' : ' (nearest match)'}
        </span>
        <button class="close" on:click={dismiss} aria-label="Dismiss proposal">×</button>
      </header>
      {#if result.base64_crop}
        <img class="crop" src={`data:image/png;base64,${result.base64_crop}`} alt="Analysed region" />
      {/if}
      <p class="hint">Review the proposal in the AI panel before applying.</p>
    </aside>
  {/if}
</div>

<style>
  .overlay-root {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    height: 100%;
    overflow: auto;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    flex-wrap: wrap;
  }
  .toggle {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    white-space: nowrap;
  }
  .instruction {
    flex: 1 1 16rem;
    min-width: 0;
    padding: 0.35rem 0.5rem;
  }
  .status {
    font-size: 0.85rem;
    opacity: 0.75;
  }
  .error {
    color: hsl(var(--err));
    margin: 0;
  }
  .page-wrap {
    position: relative;
    display: inline-block;
    line-height: 0;
  }
  .page-wrap.selecting img {
    cursor: crosshair;
  }
  .page-wrap img {
    max-width: 100%;
    height: auto;
    user-select: none;
  }
  .selection {
    position: absolute;
    border: 2px solid hsl(var(--primary));
    background: rgba(79, 70, 229, 0.15);
    pointer-events: none;
  }
  .proposal {
    border: 1px solid currentColor;
    border-radius: 6px;
    padding: 0.5rem 0.75rem;
  }
  .proposal header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .hit {
    font-family: ui-monospace, monospace;
    font-size: 0.85rem;
    opacity: 0.8;
  }
  .close {
    margin-left: auto;
    background: none;
    border: none;
    font-size: 1.2rem;
    cursor: pointer;
  }
  .crop {
    max-width: 100%;
    margin-top: 0.5rem;
    border: 1px solid rgba(128, 128, 128, 0.4);
  }
  .hint {
    margin: 0.5rem 0 0;
    font-size: 0.85rem;
    opacity: 0.75;
  }
</style>
