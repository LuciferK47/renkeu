<script>
  // Insert: figures, tables, equations, citations and cross-references, built
  // from a form so the LaTeX does not have to be typed. Nothing is written
  // from here: the chosen text goes to `oninsert`, which puts it at the cursor.
  import { untrack } from 'svelte'
  import { api } from '../lib/api/client.js'
  import { isImage } from '../lib/files.js'
  import {
    figureBlock,
    tableBlock,
    parseGrid,
    equationBlock,
    citeCommand,
    refCommand,
    labelFrom,
    uniqueLabel,
    cleanBibtex
  } from '../lib/latex/insert.js'

  let { tab: first = 'figure', project, index, ops = null, oninsert = () => {}, onclose = () => {} } = $props()

  const TABS = [
    ['figure', 'Figure'],
    ['table', 'Table'],
    ['equation', 'Equation'],
    ['citation', 'Citation'],
    ['reference', 'Reference']
  ]
  let tab = $state(untrack(() => first))
  let problem = $state('')
  let busy = $state(false)

  // What the project already contains, for labels, keys and the pickers.
  let snap = $state({ labels: [], bib: [], files: [] })
  $effect(() => {
    index
      ?.ensureLoaded()
      .then(() => (snap = index.snapshot()))
      .catch(() => {})
  })
  const takenLabels = $derived(snap.labels.map((l) => l.name))
  const images = $derived(project.files().filter((f) => isImage(f.path) && !/\.svg$/i.test(f.path) || /\.pdf$/i.test(f.path)))

  // ---- figure ----
  let figPath = $state('')
  let figCaption = $state('')
  let figLabel = $state('')
  let figLabelEdited = $state(false)
  let figWidth = $state(80)
  let figPlace = $state('htbp')
  let picker = $state()
  $effect(() => {
    if (!figPath && images.length) figPath = images[0].path
  })
  $effect(() => {
    if (!figLabelEdited) figLabel = figCaption ? uniqueLabel(labelFrom('fig', figCaption), takenLabels) : ''
  })
  async function addImage(e) {
    const files = [...e.currentTarget.files]
    e.currentTarget.value = ''
    if (!files.length || !ops) return
    if (await ops.upload(files, '')) figPath = files[0].name
  }

  // ---- table ----
  let rows = $state([['', '', ''], ['', '', ''], ['', '', '']])
  let align = $state(['l', 'l', 'l'])
  let header = $state(true)
  let rawCells = $state(false)
  let tabCaption = $state('')
  let tabLabel = $state('')
  let tabLabelEdited = $state(false)
  let paste = $state('')
  $effect(() => {
    if (!tabLabelEdited) tabLabel = tabCaption ? uniqueLabel(labelFrom('tab', tabCaption), takenLabels) : ''
  })
  const addRow = () => (rows = [...rows, rows[0].map(() => '')])
  const addCol = () => ((rows = rows.map((r) => [...r, ''])), (align = [...align, 'l']))
  const dropRow = (i) => (rows = rows.filter((_, k) => k !== i))
  const dropCol = (j) => ((rows = rows.map((r) => r.filter((_, k) => k !== j))), (align = align.filter((_, k) => k !== j)))
  function usePaste() {
    const grid = parseGrid(paste)
    if (!grid.length) return
    const width = Math.max(...grid.map((r) => r.length))
    rows = grid.map((r) => Array.from({ length: width }, (_, i) => r[i] ?? ''))
    align = Array.from({ length: width }, (_, i) => align[i] ?? 'l')
    paste = ''
  }

  // ---- equation ----
  let latex = $state('')
  let mode = $state('display')
  let eqLabel = $state('')
  let mathHost = $state()
  let field = null
  let mathReady = $state(false)
  $effect(() => {
    if (tab !== 'equation' || !mathHost || field) return
    let cancelled = false
    ;(async () => {
      try {
        const [{ MathfieldElement }] = await Promise.all([import('mathlive'), import('mathlive/fonts.css')])
        if (cancelled) return
        // The bundler serves the fonts; do not let the library fetch them or sounds itself.
        MathfieldElement.fontsDirectory = null
        MathfieldElement.soundsDirectory = null
        const mf = new MathfieldElement()
        mf.mathVirtualKeyboardPolicy = 'manual'
        mf.value = untrack(() => latex)
        mf.setAttribute('aria-label', 'Equation')
        mf.addEventListener('input', () => {
          if (mf.value !== latex) latex = mf.value
        })
        // eslint-disable-next-line svelte/no-dom-manipulating -- <math-field> is a third-party element, created after a lazy import
        mathHost.append(mf)
        field = mf
        mathReady = true
        mf.focus()
      } catch {
        // The text box below still works; only the visual editor is missing.
      }
    })()
    return () => {
      cancelled = true
    }
  })
  $effect(() => {
    const v = latex
    if (field && field.value !== v) field.value = v
  })

  // ---- citation ----
  let citeFilter = $state('')
  let picked = $state(new Set())
  let citeCmd = $state('cite')
  let doi = $state('')
  let found = $state(null)
  let bibTarget = $state('')
  const bibFiles = $derived(project.files().map((f) => f.path).filter((p) => /\.bib$/i.test(p)))
  $effect(() => {
    if (!bibTarget) bibTarget = bibFiles[0] ?? 'refs.bib'
  })
  const entries = $derived(
    snap.bib.filter((e) => `${e.key} ${e.title} ${e.author} ${e.year}`.toLowerCase().includes(citeFilter.trim().toLowerCase()))
  )
  const toggle = (key) => {
    const next = new Set(picked)
    if (!next.delete(key)) next.add(key)
    picked = next
  }
  async function lookup() {
    problem = ''
    found = null
    busy = true
    try {
      const res = await api.lookupDoi(doi)
      found = cleanBibtex(res.bibtex, snap.bib.map((b) => b.key))
      if (!found.key) problem = 'The answer did not contain a usable entry.'
    } catch (e) {
      problem = e.message
    } finally {
      busy = false
    }
  }

  // ---- reference ----
  let refFilter = $state('')
  let refCmd = $state('ref')
  const GROUPS = [
    ['Figures', 'fig'],
    ['Tables', 'tab'],
    ['Sections', 'sec'],
    ['Equations', 'eq'],
    ['Other', '']
  ]
  const groupOf = (name) => GROUPS.find(([, p]) => p && name.startsWith(`${p}:`))?.[0] ?? 'Other'
  const labelGroups = $derived(
    GROUPS.map(([title]) => [
      title,
      snap.labels.filter((l) => groupOf(l.name) === title && `${l.name} ${l.text}`.toLowerCase().includes(refFilter.trim().toLowerCase()))
    ]).filter(([, list]) => list.length)
  )

  // ---- the result ----
  const canInsert = $derived(
    tab === 'figure' ? !!figPath
    : tab === 'table' ? rows.some((r) => r.some((c) => c.trim()))
    : tab === 'equation' ? !!latex.trim()
    : tab === 'citation' ? picked.size > 0 || !!found?.key
    : false
  )

  function submit(e) {
    e.preventDefault()
    problem = ''
    if (tab === 'figure') {
      oninsert({
        text: figureBlock({ path: figPath, caption: figCaption, label: figLabel, widthPct: figWidth, placement: figPlace }),
        packages: ['graphicx'],
        block: true
      })
    } else if (tab === 'table') {
      oninsert({ text: tableBlock({ rows, header, align, caption: tabCaption, label: tabLabel, rawCells }), packages: ['booktabs'], block: true })
    } else if (tab === 'equation') {
      oninsert({ text: equationBlock({ latex, mode, label: eqLabel }), packages: mode === 'numbered' ? ['amsmath'] : [], block: mode !== 'inline' })
    } else if (tab === 'citation') {
      const keys = [...picked]
      if (found?.key) keys.push(found.key)
      oninsert({
        text: citeCommand(citeCmd, keys),
        packages: citeCmd === 'cite' ? [] : ['natbib'],
        bib: found?.key ? { path: bibTarget, text: found.text } : null
      })
    }
  }
</script>

<div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="ins-title" tabindex="-1" onkeydown={(e) => e.key === 'Escape' && onclose()}>
    <h2 id="ins-title">Insert</h2>
    <div class="tabs" role="tablist" aria-label="What to insert">
      {#each TABS as [id, label] (id)}
        <button type="button" role="tab" aria-selected={tab === id} class:on={tab === id} onclick={() => ((tab = id), (problem = ''))}>{label}</button>
      {/each}
    </div>

    <form onsubmit={submit}>
      {#if tab === 'figure'}
        <label>Image
          <span class="row">
            <select bind:value={figPath} aria-label="Image">
              {#if images.length === 0}<option value="">No images in the project yet</option>{/if}
              {#each images as f (f.path)}<option value={f.path}>{f.path}</option>{/each}
            </select>
            {#if ops}
              <button type="button" onclick={() => picker?.click()}>Add image…</button>
              <input bind:this={picker} type="file" accept="image/png,image/jpeg,.pdf" hidden onchange={addImage} />
            {/if}
          </span>
        </label>
        <label>Caption<input bind:value={figCaption} autocomplete="off" /></label>
        <label>Label<input bind:value={figLabel} oninput={() => (figLabelEdited = true)} placeholder="fig:name" autocomplete="off" spellcheck="false" /></label>
        <div class="row">
          <label class="grow">Width: {figWidth}% of the line<input type="range" min="10" max="100" step="5" bind:value={figWidth} /></label>
          <label>Placement
            <select bind:value={figPlace}>
              <option value="htbp">Wherever it fits</option>
              <option value="t">Top of a page</option>
              <option value="h!">Here</option>
              <option value="p">Own page</option>
            </select>
          </label>
        </div>
      {:else if tab === 'table'}
        <div class="grid" role="group" aria-label="Table cells">
          <div class="gridrow aligns">
            {#each align as _, j (j)}
              <span class="cell">
                <select bind:value={align[j]} aria-label={`Column ${j + 1} alignment`}><option value="l">left</option><option value="c">centre</option><option value="r">right</option></select>
                <button type="button" aria-label={`Remove column ${j + 1}`} disabled={align.length === 1} onclick={() => dropCol(j)}>×</button>
              </span>
            {/each}
          </div>
          {#each rows as row, i (i)}
            <div class="gridrow">
              {#each row as _, j (j)}
                <input bind:value={rows[i][j]} aria-label={`Row ${i + 1}, column ${j + 1}`} class:head={header && i === 0} autocomplete="off" />
              {/each}
              <button type="button" aria-label={`Remove row ${i + 1}`} disabled={rows.length === 1} onclick={() => dropRow(i)}>×</button>
            </div>
          {/each}
        </div>
        <div class="row">
          <button type="button" onclick={addRow}>+ Row</button>
          <button type="button" onclick={addCol}>+ Column</button>
          <label class="check"><input type="checkbox" bind:checked={header} /> First row is a header</label>
          <label class="check"><input type="checkbox" bind:checked={rawCells} /> Cells contain LaTeX</label>
        </div>
        <label>Paste from a spreadsheet
          <span class="row">
            <textarea bind:value={paste} rows="2" placeholder="Copy cells in Excel or Sheets and paste here"></textarea>
            <button type="button" onclick={usePaste} disabled={!paste.trim()}>Use</button>
          </span>
        </label>
        <label>Caption<input bind:value={tabCaption} autocomplete="off" /></label>
        <label>Label<input bind:value={tabLabel} oninput={() => (tabLabelEdited = true)} placeholder="tab:name" autocomplete="off" spellcheck="false" /></label>
      {:else if tab === 'equation'}
        <div class="math" bind:this={mathHost}></div>
        {#if !mathReady}<p class="note">Loading the visual editor… You can also type LaTeX below.</p>{/if}
        <label>LaTeX<textarea bind:value={latex} rows="2" spellcheck="false" aria-label="Equation LaTeX"></textarea></label>
        <div class="row">
          <label>As
            <select bind:value={mode}>
              <option value="display">Display, unnumbered</option>
              <option value="numbered">Numbered equation</option>
              <option value="inline">Inline with the text</option>
            </select>
          </label>
          {#if mode === 'numbered'}<label class="grow">Label<input bind:value={eqLabel} placeholder="eq:name" autocomplete="off" spellcheck="false" /></label>{/if}
        </div>
      {:else if tab === 'citation'}
        <label>Find in your bibliography<input bind:value={citeFilter} placeholder="Author, title, year or key" autocomplete="off" /></label>
        <ul class="list" aria-label="Bibliography entries">
          {#each entries as e (e.path + e.key)}
            <li>
              <label class="check pick">
                <input type="checkbox" checked={picked.has(e.key)} onchange={() => toggle(e.key)} />
                <span><strong>{e.key}</strong> <span class="dim">{[e.author, e.year].filter(Boolean).join(', ')}</span><br /><span class="dim">{e.title}</span></span>
              </label>
            </li>
          {:else}
            <li class="note">{snap.bib.length ? 'No entry matches.' : 'No bibliography entries yet. Add one by DOI below.'}</li>
          {/each}
        </ul>
        <fieldset>
          <legend>Add by DOI</legend>
          <span class="row">
            <input bind:value={doi} placeholder="10.1145/3292500.3330701 or a doi.org link" aria-label="DOI" autocomplete="off" spellcheck="false" />
            <button type="button" onclick={lookup} disabled={!doi.trim() || busy}>Look up</button>
          </span>
          <p class="note">This sends the DOI, and nothing else, to doi.org.</p>
          {#if found?.key}
            <pre class="bib">{found.text}</pre>
            <label>Save to
              <select bind:value={bibTarget} aria-label="Bibliography file">
                {#each bibFiles as f (f)}<option value={f}>{f}</option>{/each}
                {#if bibFiles.length === 0}<option value="refs.bib">refs.bib (new file)</option>{/if}
              </select>
            </label>
          {/if}
        </fieldset>
        <label>Command
          <select bind:value={citeCmd}>
            <option value="cite">\cite</option>
            <option value="citep">\citep (natbib, in brackets)</option>
            <option value="citet">\citet (natbib, in the sentence)</option>
          </select>
        </label>
      {:else}
        <label>Find a label<input bind:value={refFilter} placeholder="Label name or the text next to it" autocomplete="off" /></label>
        <label>Command
          <select bind:value={refCmd}>
            <option value="ref">\ref</option>
            <option value="eqref">\eqref (equations, with brackets)</option>
            <option value="cref">\cref (needs cleveref)</option>
            <option value="autoref">\autoref (needs hyperref)</option>
          </select>
        </label>
        <div class="list" aria-label="Labels">
          {#each labelGroups as [title, list] (title)}
            <h3>{title}</h3>
            <ul>
              {#each list as l (l.path + l.name)}
                <li>
                  <button type="button" class="ref" onclick={() => oninsert({ text: refCommand(refCmd, l.name), packages: refCmd === 'cref' ? ['cleveref'] : refCmd === 'autoref' ? ['hyperref'] : refCmd === 'eqref' ? ['amsmath'] : [] })}>
                    <strong>{l.name}</strong> <span class="dim">{l.path}:{l.line}</span>
                  </button>
                </li>
              {/each}
            </ul>
          {:else}
            <p class="note">{snap.labels.length ? 'No label matches.' : 'This project has no \\label yet.'}</p>
          {/each}
        </div>
      {/if}

      {#if problem}<p class="bad" role="alert">{problem}</p>{/if}
      <div class="actions">
        {#if tab !== 'reference'}<button type="submit" class="primary" disabled={!canInsert}>Insert</button>{/if}
        <button type="button" onclick={onclose}>Cancel</button>
      </div>
    </form>
  </div>
</div>

<style>
  .overlay { position: fixed; inset: 0; display: flex; justify-content: center; align-items: flex-start; padding-top: 6vh; background: rgb(0 0 0 / 0.45); z-index: 700; }
  .dialog { width: min(44rem, 94vw); max-height: 86vh; overflow-y: auto; padding: 1rem 1.1rem; background: hsl(var(--popover)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-s); box-shadow: 0 12px 40px rgb(0 0 0 / 0.4); }
  h2 { margin: 0 0 0.6rem; font-size: 17px; }
  h3 { margin: 0.6rem 0 0.15rem; font-size: 11px; text-transform: uppercase; letter-spacing: 0.05em; color: hsl(var(--text-faint)); }
  .tabs { display: flex; gap: 0.3rem; flex-wrap: wrap; margin-bottom: 0.7rem; }
  .tabs button { padding: 0.25rem 0.8rem; font: inherit; font-size: 13px; background: none; color: hsl(var(--text-dim)); border: 1px solid hsl(var(--border)); border-radius: 999px; cursor: pointer; }
  .tabs button.on { background: hsl(var(--primary)); color: hsl(var(--primary-foreground)); border-color: transparent; }
  form { display: flex; flex-direction: column; gap: 0.65rem; }
  label { display: flex; flex-direction: column; gap: 0.25rem; font-size: 13px; color: hsl(var(--text-dim)); }
  label.check { flex-direction: row; align-items: center; gap: 0.4rem; }
  .row { display: flex; align-items: flex-end; gap: 0.5rem; flex-wrap: wrap; }
  .grow { flex: 1; min-width: 8rem; }
  input, select, textarea { padding: 0.4rem 0.55rem; font: inherit; font-size: 14px; color: hsl(var(--foreground)); background: hsl(var(--surface-0)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); min-width: 0; }
  input[type='checkbox'], input[type='range'] { padding: 0; }
  textarea { resize: vertical; flex: 1; font-family: 'JetBrains Mono', monospace; font-size: 13px; }
  button { padding: 0.3rem 0.8rem; font: inherit; color: hsl(var(--secondary-foreground)); background: hsl(var(--secondary)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); cursor: pointer; }
  button.primary { color: hsl(var(--primary-foreground)); background: hsl(var(--primary)); border-color: transparent; }
  button:disabled { opacity: 0.5; cursor: default; }
  .actions { display: flex; gap: 0.5rem; }
  .grid { display: flex; flex-direction: column; gap: 0.25rem; overflow-x: auto; }
  .gridrow { display: flex; gap: 0.25rem; align-items: center; }
  .gridrow input { flex: 1; min-width: 5rem; }
  .gridrow input.head { font-weight: 700; }
  .aligns .cell { flex: 1; min-width: 5rem; display: flex; gap: 0.2rem; }
  .aligns select { flex: 1; font-size: 12px; padding: 0.2rem; }
  .gridrow > button, .aligns button { padding: 0.1rem 0.45rem; }
  .math { min-height: 3.2rem; }
  .math :global(math-field) { width: 100%; font-size: 22px; padding: 0.3rem 0.5rem; background: hsl(var(--surface-0)); color: hsl(var(--foreground)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); }
  .list { max-height: 14rem; overflow-y: auto; margin: 0; padding: 0; list-style: none; border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); }
  .list ul { list-style: none; margin: 0; padding: 0; }
  .list li { padding: 0.25rem 0.5rem; }
  .pick { align-items: flex-start; }
  .ref { display: block; width: 100%; text-align: left; background: none; border: none; padding: 0.2rem 0.3rem; color: inherit; }
  .ref:hover, .ref:focus-visible { background: hsl(var(--accent)); outline: none; }
  .dim { color: hsl(var(--text-faint)); font-size: 12px; }
  fieldset { display: flex; flex-direction: column; gap: 0.4rem; margin: 0; padding: 0.5rem 0.6rem; border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); }
  legend { padding: 0 0.3rem; font-size: 13px; color: hsl(var(--text-dim)); }
  fieldset .row input { flex: 1; }
  .bib { margin: 0; padding: 0.5rem; max-height: 8rem; overflow: auto; font-size: 12px; background: hsl(var(--surface-0)); border-radius: var(--radius-xs); white-space: pre-wrap; }
  .note { margin: 0; font-size: 12px; color: hsl(var(--text-dim)); }
  .bad { margin: 0; font-size: 13px; color: hsl(var(--err)); }
</style>
