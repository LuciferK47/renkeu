<script>
  // Start from a template, or edit the details of a document made from one.
  //
  // 'new': pick a template from the gallery, fill in the details, create the
  // files in the (empty) project. 'details': skip the gallery and change the
  // title, authors and so on of the open document; the caller swaps the
  // re-rendered block into the editor, so it is one undoable edit.
  import { untrack } from 'svelte'
  import { api } from '../lib/api/client.js'
  import { emptyFields } from '../lib/templates.js'

  let { mode = 'new', initial = null, oncreated = () => {}, onapply = () => {}, onclose = () => {} } = $props()

  const seed = untrack(() => initial)
  let templates = $state([])
  let loadError = $state('')
  let chosen = $state(seed?.template ?? null)
  let category = $state('All')
  let fields = $state(seed ? { ...emptyFields(), ...seed.fields } : emptyFields())
  let busy = $state(false)
  let problem = $state('')

  const info = $derived(templates.find((t) => t.id === chosen) ?? null)
  const categories = $derived(['All', ...new Set(templates.map((t) => t.category))])
  const shown = $derived(templates.filter((t) => category === 'All' || t.category === category))
  const uses = (name) => info?.fields.includes(name) ?? false

  $effect(() => {
    api
      .getTemplates()
      .then((r) => (templates = r.templates))
      .catch((e) => (loadError = e.message))
  })

  function choose(id) {
    chosen = id
    problem = ''
  }

  const addAuthor = () => (fields.authors = [...fields.authors, { name: '', affiliation: '', email: '' }])
  const removeAuthor = (i) => (fields.authors = fields.authors.filter((_, k) => k !== i))

  async function submit(e) {
    e.preventDefault()
    if (!info || busy) return
    busy = true
    problem = ''
    try {
      if (mode === 'details') {
        const { block } = await api.renderFrontmatter(info.id, fields)
        onapply(block)
      } else {
        const res = await api.startFromTemplate(info.id, fields)
        oncreated(res.main)
      }
    } catch (err) {
      problem = err.status === 409 && mode === 'new' ? 'This project already has a .tex file. Open an empty folder to start from a template.' : err.message
    } finally {
      busy = false
    }
  }
</script>

<div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="tpl-title" tabindex="-1" onkeydown={(e) => e.key === 'Escape' && onclose()}>
    {#if !info}
      <h2 id="tpl-title">Start from a template</h2>
      {#if loadError}<p class="bad" role="alert">{loadError}</p>{/if}
      <div class="cats" role="tablist" aria-label="Template category">
        {#each categories as c (c)}
          <button type="button" role="tab" aria-selected={category === c} class:on={category === c} onclick={() => (category = c)}>{c}</button>
        {/each}
      </div>
      <ul class="grid">
        {#each shown as t (t.id)}
          <li>
            <button type="button" class="card" onclick={() => choose(t.id)}>
              <strong>{t.name}</strong>
              <span class="cat">{t.category}</span>
              <span class="desc">{t.description}</span>
            </button>
          </li>
        {/each}
      </ul>
      <div class="actions"><button type="button" onclick={onclose}>Cancel</button></div>
    {:else}
      <form onsubmit={submit}>
        <h2 id="tpl-title">{mode === 'details' ? 'Document details' : info.name}</h2>
        {#if mode === 'details'}
          <p class="note">Saving rewrites the title block of this document ({info.name}). Anything you changed by hand inside that block is replaced; you can undo it.</p>
        {/if}

        {#if uses('title')}
          <label>Title<input bind:value={fields.title} autocomplete="off" /></label>
        {/if}

        {#if uses('authors')}
          <fieldset>
            <legend>{info.id === 'letter' ? 'Sender' : 'Authors'}</legend>
            {#each fields.authors as a, i (i)}
              <div class="author">
                <input bind:value={a.name} placeholder="Name" aria-label={`Author ${i + 1} name`} autocomplete="off" />
                <input bind:value={a.affiliation} placeholder="Affiliation or address" aria-label={`Author ${i + 1} affiliation`} autocomplete="off" />
                <input bind:value={a.email} placeholder="Email" aria-label={`Author ${i + 1} email`} autocomplete="off" />
                <button type="button" aria-label={`Remove author ${i + 1}`} disabled={fields.authors.length === 1} onclick={() => removeAuthor(i)}>×</button>
              </div>
            {/each}
            <button type="button" class="add" onclick={addAuthor}>+ Add another</button>
          </fieldset>
        {/if}

        {#if uses('abstract')}
          <label>Abstract<textarea bind:value={fields.abstract} rows="4"></textarea></label>
        {/if}
        {#if uses('keywords')}
          <label>Keywords<input bind:value={fields.keywords} placeholder="comma, separated" autocomplete="off" /></label>
        {/if}
        {#if uses('date')}
          <label>Date<input bind:value={fields.date} placeholder="Leave empty for today" autocomplete="off" /></label>
        {/if}

        {#if problem}<p class="bad" role="alert">{problem}</p>{/if}
        <div class="actions">
          {#if mode === 'new'}<button type="button" onclick={() => (chosen = null)}>Back</button>{/if}
          <button type="submit" class="primary" disabled={busy}>{mode === 'details' ? 'Update document' : 'Create project'}</button>
          <button type="button" onclick={onclose}>Cancel</button>
        </div>
      </form>
    {/if}
  </div>
</div>

<style>
  .overlay { position: fixed; inset: 0; display: flex; justify-content: center; align-items: flex-start; padding-top: 8vh; background: rgb(0 0 0 / 0.45); z-index: 700; }
  .dialog { width: min(46rem, 94vw); max-height: 82vh; overflow-y: auto; padding: 1rem 1.1rem; background: hsl(var(--popover)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-s); box-shadow: 0 12px 40px rgb(0 0 0 / 0.4); }
  h2 { margin: 0 0 0.7rem; font-size: 17px; }
  .cats { display: flex; gap: 0.35rem; flex-wrap: wrap; margin-bottom: 0.7rem; }
  .cats button { padding: 0.2rem 0.7rem; font: inherit; font-size: 13px; background: none; color: hsl(var(--text-dim)); border: 1px solid hsl(var(--border)); border-radius: 999px; cursor: pointer; }
  .cats button.on { background: hsl(var(--primary)); color: hsl(var(--primary-foreground)); border-color: transparent; }
  .grid { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: repeat(auto-fill, minmax(13rem, 1fr)); gap: 0.6rem; }
  .card { display: flex; flex-direction: column; gap: 0.25rem; width: 100%; height: 100%; padding: 0.7rem; text-align: left; font: inherit; color: inherit; background: hsl(var(--surface-0)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); cursor: pointer; }
  .card:hover, .card:focus-visible { border-color: hsl(var(--primary)); outline: none; }
  .cat { font-size: 11px; text-transform: uppercase; letter-spacing: 0.06em; color: hsl(var(--text-faint)); }
  .desc { font-size: 13px; color: hsl(var(--text-dim)); }
  form { display: flex; flex-direction: column; gap: 0.7rem; }
  label { display: flex; flex-direction: column; gap: 0.25rem; font-size: 13px; color: hsl(var(--text-dim)); }
  input, textarea { padding: 0.45rem 0.6rem; font: inherit; font-size: 14px; color: hsl(var(--foreground)); background: hsl(var(--surface-0)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); }
  textarea { resize: vertical; }
  fieldset { display: flex; flex-direction: column; gap: 0.4rem; margin: 0; padding: 0.5rem 0.6rem; border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); }
  legend { padding: 0 0.3rem; font-size: 13px; color: hsl(var(--text-dim)); }
  .author { display: grid; grid-template-columns: 1fr 1.3fr 1fr auto; gap: 0.4rem; }
  .add { align-self: flex-start; background: none; border: none; color: hsl(var(--primary)); cursor: pointer; font: inherit; font-size: 13px; padding: 0.1rem 0; }
  .note { margin: 0; font-size: 13px; color: hsl(var(--text-dim)); }
  .bad { margin: 0; font-size: 13px; color: hsl(var(--err)); }
  .actions { display: flex; gap: 0.5rem; margin-top: 0.3rem; }
  .actions button, .author button { padding: 0.3rem 0.8rem; font: inherit; border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); background: hsl(var(--secondary)); color: hsl(var(--secondary-foreground)); cursor: pointer; }
  .actions .primary { background: hsl(var(--primary)); color: hsl(var(--primary-foreground)); border-color: transparent; }
  button:disabled { opacity: 0.5; cursor: default; }
  @media (max-width: 40rem) { .author { grid-template-columns: 1fr 1fr; } }
</style>
