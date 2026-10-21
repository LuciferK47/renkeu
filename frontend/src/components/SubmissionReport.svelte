<script>
  // Is it ready to submit? Pick a profile (arXiv packaging, double-blind,
  // accessibility, or one of your own in <home>/profiles) and see what it
  // finds in the project and the PDF. Each finding with a line is a link to it.
  // The rules belong to the profile, which names where they come from; a clean
  // report means nothing was found, not that the venue will accept it.
  import { api } from '../lib/api/client.js'
  import { readPref, writePref } from '../lib/storage.js'

  let { compile, onreveal = () => {} } = $props()

  const KEY = 'latex-ai:preflight-profile'
  let profiles = $state([])
  let chosen = $state(readPref(KEY, ''))
  let report = $state(null)
  let error = $state('')
  let loading = $state(false)

  const profile = $derived(profiles.find((p) => p.id === chosen) ?? null)

  $effect(() => {
    api
      .getPreflightProfiles()
      .then((r) => {
        profiles = r.profiles
        if (!r.profiles.some((p) => p.id === chosen)) chosen = r.profiles[0]?.id ?? ''
      })
      .catch((e) => (error = e.message))
  })

  // Ask again when the build or the profile changes; an older answer never
  // replaces a newer one.
  let ticket = 0
  $effect(() => {
    const id = compile.buildId
    const wanted = chosen
    const mine = ++ticket
    if (!id || !wanted) {
      report = null
      return
    }
    loading = true
    api
      .getPreflight(id, wanted)
      .then((r) => {
        if (mine === ticket) (report = r, (error = ''))
      })
      .catch((e) => {
        if (mine === ticket) (report = null, (error = e.message))
      })
      .finally(() => mine === ticket && (loading = false))
  })

  function choose(id) {
    chosen = id
    writePref(KEY, id)
  }

  const label = { error: 'Error', warning: 'Warning', info: 'Note' }
  const counts = $derived.by(() => {
    const c = { error: 0, warning: 0, info: 0 }
    for (const f of report?.findings ?? []) c[f.severity] += 1
    return c
  })
</script>

<div class="submission">
  <div class="row">
    <label>
      Check for
      <select value={chosen} onchange={(e) => choose(e.currentTarget.value)} aria-label="Profile">
        {#each profiles as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
      </select>
    </label>
    {#if report}
      <span class="verdict" class:ok={report.passed} class:bad={!report.passed}>
        {report.passed ? 'Nothing blocking' : `${counts.error} to fix`}
        {#if counts.warning}· {counts.warning} to look at{/if}
      </span>
    {/if}
  </div>

  {#if profile}
    <p class="note">{profile.description}</p>
    {#if profile.source}<p class="note">Rules from <a href={profile.source} target="_blank" rel="noreferrer noopener">{profile.source}</a></p>{/if}
  {/if}

  {#if !compile.buildId}
    <p class="note">Compile to check the project against a profile.</p>
  {:else if error}
    <p class="note bad" role="alert">Could not run the checks: {error}</p>
  {:else if loading && !report}
    <p class="note">Checking…</p>
  {:else if report}
    {#if report.stale}<p class="note">The PDF is older than your edits; the page and font checks are for the PDF you see.</p>{/if}
    {#each report.findings as f, i (i)}
      <button class="item {f.severity}" disabled={!f.file} onclick={() => f.file && onreveal({ file: f.file, line: f.line ?? 1 })}>
        <span class="tag">{label[f.severity]}</span>
        <span class="msg">{f.message}</span>
        {#if f.file}<span class="loc">{f.file}{f.line ? `:${f.line}` : ''}</span>{/if}
      </button>
    {:else}
      <p class="note ok">No findings.</p>
    {/each}
  {/if}
</div>

<style>
  .submission { padding: 0.4rem 0 0.8rem; }
  .row { display: flex; align-items: center; gap: 1rem; padding: 0.2rem 0.75rem 0.4rem; flex-wrap: wrap; }
  label { display: flex; align-items: center; gap: 0.4rem; color: hsl(var(--text-dim)); }
  select { padding: 0.2rem 0.4rem; font: inherit; color: hsl(var(--foreground)); background: hsl(var(--surface-0)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); }
  .verdict { font-weight: 600; }
  .verdict.ok { color: hsl(var(--ok)); }
  .verdict.bad { color: hsl(var(--err)); }
  .note { margin: 0; padding: 0.15rem 0.75rem; color: hsl(var(--text-dim)); }
  .note.ok { color: hsl(var(--ok)); }
  .note.bad { color: hsl(var(--err)); }
  .note a { color: inherit; }
  .item { display: flex; gap: 0.6rem; align-items: baseline; width: 100%; text-align: left; padding: 0.25rem 0.75rem; background: none; border: none; font: inherit; color: hsl(var(--foreground)); cursor: pointer; }
  .item:hover:not(:disabled) { background: hsl(var(--accent)); }
  .item:disabled { cursor: default; }
  .tag { width: 4.2rem; flex-shrink: 0; font-size: 11px; text-transform: uppercase; letter-spacing: 0.05em; color: hsl(var(--text-faint)); }
  .item.error .tag { color: hsl(var(--err)); }
  .item.warning .tag { color: hsl(var(--warn, var(--text-dim))); }
  .msg { flex: 1; }
  .loc { color: hsl(var(--text-dim)); font-family: 'JetBrains Mono', monospace; font-size: 12px; white-space: nowrap; }
</style>
