<script>
  // The courier crab, drawn from the same pixel map the landing page extrudes
  // into voxels (lib/sprites/crab.js). `scale` is screen pixels per sprite
  // pixel, kept an integer so the edges stay crisp.
  import { crabCells } from './sprites/crab.js'

  let { pose = 'idle', scale = 4, walking = false, label = '' } = $props()

  let frame = $state(0)
  $effect(() => {
    if (!walking || globalThis.matchMedia?.('(prefers-reduced-motion: reduce)').matches) {
      frame = 0
      return
    }
    const t = setInterval(() => (frame = (frame + 1) % 2), 280)
    return () => clearInterval(t)
  })

  const cells = $derived(crabCells(pose, frame))
</script>

<svg
  width={14 * scale}
  height={10 * scale}
  viewBox="0 0 14 10"
  shape-rendering="crispEdges"
  role={label ? 'img' : undefined}
  aria-label={label || undefined}
  aria-hidden={label ? undefined : 'true'}
  class="crab"
>
  {#each cells as c (`${c.x},${c.y}`)}<rect x={c.x} y={c.y} width="1" height="1" fill={c.fill} />{/each}
</svg>

<style>
  .crab { display: block; image-rendering: pixelated; }
</style>
