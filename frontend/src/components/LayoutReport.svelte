<script>
  // What the compiled document looks like on the page, in numbers: how many
  // pages, where a page budget is crossed, the boxes TeX complained about, the
  // paragraphs that end in a word or two, and fonts left unembedded. Read from
  // the build, no AI involved. Items point at the place on the PDF or the line.
  import { api } from '../lib/api/client.js'
  import { readPref, writePref } from '../lib/storage.js'

  let { compile, onshow = () => {}, onreveal = () => {}, onfit = null } = $props()

  const BUDGET_KEY = 'latex-ai:page-budget'
  let budget = $state(readPref(BUDGET_KEY, ''))
  let report = $state(null)
  let error = $state('')
  let loading = $state(false)

  const limit = $derived.by(() => {
    const n = Number.parseInt(String(budget), 10)
    return Number.isInteger(n) && n >= 1 && n <= 10000 ? n : null
  })

  // Ask again when the build or the budget changes; an older answer never
  // replaces a newer one.
  let ticket = 0
  $effect(() => {
    const id = compile.buildId
    const pages = limit
    const mine = ++ticket
    if (!id || compile.status !== 'success') {
      report = null
      error = ''
      return
    }
    loading = true
    const timer = setTimeout(async () => {
      try {
        const found = await api.getLayout(id, pages)
        if (mine === ticket) (report = found, (error = ''))
      } catch (e) {
        if (mine === ticket) (report = null, (error = e.message))
      } finally {
        if (mine === ticket) loading = false
      }
    }, 150)
    return () => clearTimeout(timer)
  })

  function setBudget(value) {
    budget = value
    writePref(BUDGET_KEY, value)
  }

  const where = (p) => `${p.file}:${p.line}`
  const plural = (n, word) => `${n} ${word}${n === 1 ? '' : 's'}`

  const summary = $derived.by(() => {
    if (!report) return ''
    const parts = [plural(report.pages, 'page')]
    if (report.fill.ratio !== null) parts.push(`last page ${Math.round(report.fill.ratio * 100)}% as full as the fullest`)
    return parts.join(' · ')
  })
  const boxLabel = (b) => {
    const what = `${b.overfull ? 'Overfull' : 'Underfull'} ${b.horizontal ? 'line' : 'page'}`
    return b.amount_pt === null ? what : `${what} by ${b.amount_pt.toFixed(1)} pt`
  }
</script>

<div class="layout">
  <div class="row">
    <label>
      Page limit
      <input type="number" min="1" max="10000" inputmode="numeric" value={budget} oninput={(e) => setBudget(e.currentTarget.value)} placeholder="none" aria-label="Page limit" />
    </label>
    {#if report}<span class="summary">{summary}</span>{/if}
  </div>

  {#if !compile.buildId || compile.status !== 'success'}
    <p class="note">Compile successfully to see the layout.</p>
  {:else if error}
    <p class="note bad" role="alert">Could not read the layout: {error}</p>
  {:else if !report}
    <p class="note">{loading ? 'Reading the layout…' : ''}</p>
  {:else}
    {#if report.stale}<p class="note">The PDF is older than your edits; positions are for the PDF you see.</p>{/if}

    {#if report.budget}
      {#if report.budget.over}
        <h3>Over the limit</h3>
        <p class="note bad">
          {plural(report.pages, 'page')} against a limit of {report.budget.limit}: about {plural(report.budget.lines_over, 'line')} too many.
        </p>
        {#if onfit}
          <button class="fit" onclick={() => onfit(report.budget.limit)}>Fit to {plural(report.budget.limit, 'page')} with the assistant…</button>
        {/if}
        {#if report.budget.first_over}
          {@const first = report.budget.first_over}
          <button class="item" onclick={() => onshow(first)}>
            <span class="msg">First line past page {report.budget.limit}</span>
            <span class="loc">page {first.page}</span>
          </button>
          <button class="item" onclick={() => onreveal({ file: first.file, line: first.line })}>
            <span class="msg">Its source (the paragraph ends here)</span>
            <span class="loc">{where(first)}</span>
          </button>
        {/if}
      {:else}
        <p class="note ok">Within the limit: {report.pages} of {report.budget.limit} pages.</p>
      {/if}
    {/if}

    <h3>Boxes TeX complained about</h3>
    {#each report.boxes as b, i (i)}
      <button class="item" disabled={!b.place} onclick={() => b.place && onshow(b.place)}>
        <span class="msg">{boxLabel(b)}</span>
        {#if b.place}<span class="loc">{where(b.place)}</span>{/if}
      </button>
    {:else}
      <p class="note ok">No overfull or underfull boxes.</p>
    {/each}
    {#if report.boxes_total > report.boxes.length}<p class="note">{report.boxes_total - report.boxes.length} more not listed.</p>{/if}

    <h3>Paragraphs ending in a very short line</h3>
    <p class="note">Candidates: cutting a word or two in these saves a whole line.</p>
    {#each report.short_last_lines as s, i (i)}
      <button class="item" onclick={() => onshow(s)}>
        <span class="msg">Page {s.page}</span>
        <span class="loc">{where(s)}</span>
      </button>
    {:else}
      <p class="note ok">None found.</p>
    {/each}
    {#if report.short_last_lines_total > report.short_last_lines.length}<p class="note">{report.short_last_lines_total - report.short_last_lines.length} more not listed.</p>{/if}

    {#if report.fonts_not_embedded}
      <h3>Fonts</h3>
      {#if report.fonts_not_embedded.length}
        <p class="note bad">Not embedded: {report.fonts_not_embedded.join(', ')}. Many publishers require every font to be embedded.</p>
      {:else}
        <p class="note ok">All fonts are embedded.</p>
      {/if}
    {/if}
  {/if}
</div>

<style>
  .layout { padding: 0.4rem 0 0.8rem; }
  .row { display: flex; align-items: center; gap: 1rem; padding: 0.2rem 0.75rem 0.4rem; flex-wrap: wrap; }
  label { display: flex; align-items: center; gap: 0.4rem; color: hsl(var(--text-dim)); }
  input { width: 5rem; padding: 0.2rem 0.4rem; font: inherit; color: hsl(var(--foreground)); background: hsl(var(--surface-0)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); }
  .summary { color: hsl(var(--foreground)); font-weight: 600; }
  h3 { margin: 0.5rem 0.75rem 0.1rem; font-size: 11px; text-transform: uppercase; letter-spacing: 0.05em; color: hsl(var(--text-faint)); }
  .item { display: flex; gap: 0.5rem; align-items: baseline; width: 100%; text-align: left; padding: 0.25rem 0.75rem; background: none; border: none; font: inherit; color: hsl(var(--foreground)); cursor: pointer; }
  .item:hover:not(:disabled) { background: hsl(var(--accent)); }
  .item:disabled { cursor: default; }
  .fit { margin: 0.3rem 0.75rem; padding: 0.25rem 0.7rem; font: inherit; color: hsl(var(--primary-foreground)); background: hsl(var(--primary)); border: none; border-radius: var(--radius-xs); cursor: pointer; }
  .msg { flex: 1; }
  .loc { color: hsl(var(--text-dim)); font-family: 'JetBrains Mono', monospace; font-size: 12px; white-space: nowrap; }
  .note { margin: 0; padding: 0.15rem 0.75rem; color: hsl(var(--text-dim)); }
  .note.ok { color: hsl(var(--ok)); }
  .note.bad { color: hsl(var(--err)); }
</style>
