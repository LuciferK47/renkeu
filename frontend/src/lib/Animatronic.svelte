<script>
  // One of the foundry crew, drawn from the same pixel maps the landing scene
  // extrudes into voxels (lib/sprites/crew.js). `animate` flips between the two
  // frames slowly, and only when motion is welcome.
  import { crewCells, CREW_SIZE } from './sprites/crew.js'

  let { id = 'bruno', scale = 4, animate = false, label = '' } = $props()

  let frame = $state(0)
  $effect(() => {
    if (!animate || globalThis.matchMedia?.('(prefers-reduced-motion: reduce)').matches) {
      frame = 0
      return
    }
    const t = setInterval(() => (frame = (frame + 1) % 2), 700)
    return () => clearInterval(t)
  })

  const cells = $derived(crewCells(id, frame))
</script>

<svg
  width={CREW_SIZE.width * scale}
  height={CREW_SIZE.height * scale}
  viewBox={`0 0 ${CREW_SIZE.width} ${CREW_SIZE.height}`}
  shape-rendering="crispEdges"
  role={label ? 'img' : undefined}
  aria-label={label || undefined}
  aria-hidden={label ? undefined : 'true'}
  class="crew"
>
  {#each cells as c (`${c.x},${c.y}`)}<rect x={c.x} y={c.y} width="1" height="1" fill={c.fill} />{/each}
</svg>

<style>
  .crew { display: block; image-rendering: pixelated; flex: none; }
</style>
