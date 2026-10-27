<script>
  // The fixed 3D background. It renders live when the device can, and falls back
  // to still frames of each floor otherwise. All the content stays in the DOM:
  // this layer is decoration and is hidden from assistive technology.
  let {
    tier = 'poster',
    position = 0,
    input = {},
    paused = false,
    floorCount = 7,
    onready = () => {},
    onfailed = () => {}
  } = $props()

  let canvas = $state()
  let foundry = $state.raw(null)
  let failed = $state(false)

  // Runs again when the tier is decided (the page starts as 'poster' and picks
  // a tier once it can look at the device) and when the canvas appears.
  $effect(() => {
    if (tier === 'poster' || !canvas) return
    const target = canvas
    const chosen = tier
    let cancelled = false
    let off = () => {}
    ;(async () => {
      try {
        const { createFoundry } = await import('./scene/foundry.js')
        if (cancelled) return
        foundry = createFoundry({ canvas: target, tier: chosen })
        foundry.start()
        if (import.meta.env.DEV) window.__foundry = foundry
        const io = new IntersectionObserver(([e]) => foundry?.setVisible(e.isIntersecting))
        io.observe(target)
        const move = (e) => foundry?.setPointer((e.clientX / innerWidth) * 2 - 1, -((e.clientY / innerHeight) * 2 - 1))
        window.addEventListener('pointermove', move, { passive: true })
        off = () => {
          io.disconnect()
          window.removeEventListener('pointermove', move)
        }
        onready(foundry)
      } catch (err) {
        // No WebGL after all, or the context was refused: the posters take over.
        failed = true
        onfailed(err)
      }
    })()
    return () => {
      cancelled = true
      off()
      foundry?.dispose()
      foundry = null
    }
  })

  $effect(() => foundry?.setPosition(position))
  $effect(() => foundry?.setInput($state.snapshot(input)))
  $effect(() => foundry?.setVisible(!paused))

  const showPoster = $derived(tier === 'poster' || failed)
  const floor = $derived(Math.min(floorCount - 1, Math.max(0, Math.round(position))))
</script>

<div class="scene" aria-hidden="true">
  {#if showPoster}
    <div class="posters">
      {#each Array(floorCount) as _, i (i)}
        <img class:on={i === floor} src={`${import.meta.env.BASE_URL}landing/poster-${i}.jpg`} alt="" loading={i === 0 ? 'eager' : 'lazy'} decoding="async" onerror={(e) => (e.currentTarget.hidden = true)} />
      {/each}
    </div>
  {:else}
    <canvas bind:this={canvas}></canvas>
  {/if}
  <div class="grain"></div>
</div>

<style>
  .scene { position: fixed; inset: 0; z-index: 0; background: #07080b; overflow: hidden; pointer-events: none; }
  canvas { position: absolute; inset: 0; width: 100%; height: 100%; image-rendering: pixelated; display: block; }
  .posters img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; image-rendering: pixelated; opacity: 0; transition: opacity 300ms steps(6, end); }
  .posters img.on { opacity: 1; }
  .grain { position: absolute; inset: 0; background: repeating-linear-gradient(0deg, rgba(0, 0, 0, 0.16) 0 1px, transparent 1px 3px); mix-blend-mode: multiply; opacity: 0.5; }
  @media (prefers-reduced-motion: reduce) { .posters img { transition: none; } }
</style>
