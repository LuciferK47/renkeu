<script>
  // A transparent layer over one page for drawing a box. The box is reported
  // in PDF points, so it means the same thing at any zoom; while drawing it is
  // in screen px, and a finished selection is redrawn from points so it stays
  // on the same text when the zoom changes.
  import { normalizeDrag, isMeaningfulDrag, toPagePoints, fromPagePoints } from '../utils/bbox.js'

  let { page, pxPerPt, selection = null, disabled = false, onselect = () => {} } = $props()

  let layer = $state()
  let start = null
  let drag = $state(null)

  const shown = $derived(
    drag ?? (selection && selection.page === page ? fromPagePoints(selection, pxPerPt) : null)
  )

  const local = (e) => {
    const r = layer.getBoundingClientRect()
    return { x: e.clientX - r.left, y: e.clientY - r.top }
  }
  const bounds = () => ({ width: layer.clientWidth, height: layer.clientHeight })

  function down(e) {
    if (disabled || e.button !== 0) return
    layer.setPointerCapture?.(e.pointerId)
    start = local(e)
    drag = { ...start, w: 0, h: 0 }
  }

  function move(e) {
    if (start) drag = normalizeDrag(start, local(e), bounds())
  }

  function up(e) {
    if (!start) return
    layer.releasePointerCapture?.(e.pointerId)
    const box = drag
    start = null
    drag = null
    if (isMeaningfulDrag(box)) onselect({ page, ...toPagePoints(box, pxPerPt) })
  }
</script>

<div
  class="layer"
  class:disabled
  bind:this={layer}
  role="presentation"
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={() => ((start = null), (drag = null))}
>
  {#if shown}
    <div class="box" style:left={`${shown.x}px`} style:top={`${shown.y}px`} style:width={`${shown.w}px`} style:height={`${shown.h}px`}></div>
  {/if}
</div>

<style>
  .layer { position: absolute; inset: 0; cursor: crosshair; touch-action: none; }
  .layer.disabled { cursor: progress; }
  .box { position: absolute; border: 2px solid hsl(var(--primary)); background: hsl(var(--primary) / 0.15); pointer-events: none; }
</style>
