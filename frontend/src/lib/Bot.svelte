<script>
  // The repair bot, from the shared pixel map (lib/sprites/bot.js). Asleep when
  // the assistant is off, working while it runs. Decorative unless labelled.
  import { botFrame, BOT_PALETTE } from './sprites/bot.js'

  let { mood = 'idle', scale = 4, label = '' } = $props()

  const cells = $derived(
    botFrame(mood).flatMap((row, y) =>
      [...row].map((ch, x) => (ch === '.' ? null : { x, y, fill: BOT_PALETTE[ch] })).filter(Boolean)
    )
  )
</script>

<svg
  width={11 * scale}
  height={10 * scale}
  viewBox="0 0 11 10"
  shape-rendering="crispEdges"
  role={label ? 'img' : undefined}
  aria-label={label || undefined}
  aria-hidden={label ? undefined : 'true'}
  class="bot"
  class:working={mood === 'working'}
>
  {#each cells as c (`${c.x},${c.y}`)}<rect x={c.x} y={c.y} width="1" height="1" fill={c.fill} />{/each}
</svg>

<style>
  .bot { display: block; image-rendering: pixelated; }
  .working { animation: bob 0.8s steps(2, end) infinite; }
  @keyframes bob { 50% { transform: translateY(-2px); } }
  @media (prefers-reduced-motion: reduce) { .working { animation: none; } }
</style>
