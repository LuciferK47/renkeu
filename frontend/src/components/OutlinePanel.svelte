<script>
  // The document's structure, from its section commands. Clicking a heading
  // moves the editor there. Rebuilt a moment after typing stops.
  import { scan } from '../lib/latex/scan.js'

  let { documents, onreveal = () => {} } = $props()

  let sections = $state([])
  let collapsed = $state(false)

  $effect(() => {
    const doc = documents.active
    if (!doc || !/\.(tex|sty|cls)$/i.test(doc.path)) {
      sections = []
      return
    }
    const build = () => (sections = scan(doc.text).sections)
    build()
    let timer
    const sub = doc.model.onDidChangeContent?.(() => {
      clearTimeout(timer)
      timer = setTimeout(build, 300)
    })
    return () => {
      clearTimeout(timer)
      sub?.dispose?.()
    }
  })

  const shallowest = $derived(sections.length ? Math.min(...sections.map((s) => s.level)) : 0)
</script>

<section class="outline" aria-label="Outline">
  <header>
    <button class="toggle" aria-expanded={!collapsed} onclick={() => (collapsed = !collapsed)}>
      <span aria-hidden="true">{collapsed ? '▸' : '▾'}</span> Outline
    </button>
  </header>
  {#if !collapsed}
    {#if sections.length === 0}
      <p class="note">No headings in this file.</p>
    {:else}
      <ul>
        {#each sections as s (s.line + s.command)}
          <li>
            <button
              style:padding-left={`${0.75 + (s.level - shallowest) * 0.8}rem`}
              class:starred={s.starred}
              title={`Line ${s.line}`}
              onclick={() => onreveal(s.line)}
            >{s.title || `(untitled ${s.command})`}</button>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</section>

<style>
  .outline { display: flex; flex-direction: column; min-height: 0; max-height: 45%; border-top: 1px solid hsl(var(--border)); background: hsl(var(--surface-1)); font-size: 13px; }
  header { padding: 0.35rem 0.5rem; }
  .toggle { background: none; border: none; font: inherit; font-weight: 600; font-size: 11px; letter-spacing: 0.04em; text-transform: uppercase; color: hsl(var(--text-dim)); cursor: pointer; }
  ul { list-style: none; margin: 0; padding: 0 0 0.4rem; overflow: auto; }
  li button { display: block; width: 100%; text-align: left; padding: 0.2rem 0.5rem; background: none; border: none; font: inherit; color: hsl(var(--foreground)); cursor: pointer; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  li button:hover { background: hsl(var(--accent)); }
  .starred { font-style: italic; }
  .note { margin: 0; padding: 0 0.75rem 0.6rem; color: hsl(var(--text-dim)); }
</style>
