<script>
  // The project tree and its file manager. Directories expand and collapse;
  // clicking an editable file opens it and an image previews. Right-click (or
  // the Menu key) opens file actions; files can be dropped in to upload.
  import NamePrompt from './NamePrompt.svelte'
  import ImagePreview from './ImagePreview.svelte'
  import { isImage, fileGroup, FILE_GROUPS } from '../lib/files.js'

  let {
    project,
    documents,
    ops = null,
    view = 'folders',
    mainFile = null,
    errorFiles = new Set(),
    onview = () => {},
    onopen = () => {},
    onrefresh = () => {}
  } = $props()

  /** { x, y, node } while the context menu is open; node null = the root. */
  let menu = $state(null)
  /** { kind: 'create' | 'rename', dir?, node? } while asking for a name. */
  let prompt = $state(null)
  let previewPath = $state(null)
  let dropTarget = $state(null)
  let picker = $state()

  // Flatten to the rows that are visible given what is expanded, or, in the
  // "By type" view, to the files grouped under Documents, Bibliography, ….
  const rows = $derived.by(() => {
    if (view === 'type') {
      const byGroup = new Map(FILE_GROUPS.map((g) => [g, []]))
      for (const file of project.files()) byGroup.get(fileGroup(file.path)).push(file)
      const grouped = []
      for (const [group, files] of byGroup) {
        if (files.length === 0) continue
        grouped.push({ group, key: `group:${group}` })
        for (const node of files.sort((a, b) => a.path.localeCompare(b.path))) grouped.push({ node, depth: 0, key: node.path })
      }
      return grouped
    }
    const out = []
    const walk = (nodes, depth) => {
      for (const node of nodes) {
        out.push({ node, depth, key: node.path })
        if (node.kind === 'dir' && project.expanded.has(node.path)) walk(node.children ?? [], depth + 1)
      }
    }
    walk(project.tree, 0)
    return out
  })

  const openable = (node) => node.kind === 'file' && (node.editable || isImage(node.path))

  function activate(node) {
    if (node.kind === 'dir') project.toggle(node.path)
    else if (node.editable) onopen(node.path)
    else if (isImage(node.path)) previewPath = node.path
  }

  const parentOf = (path) => (path.includes('/') ? path.slice(0, path.lastIndexOf('/')) : '')
  const dirFor = (node) => (!node ? '' : node.kind === 'dir' ? node.path : parentOf(node.path))

  function openMenu(e, node) {
    if (!ops) return
    e.preventDefault()
    const box = e.currentTarget.getBoundingClientRect()
    menu = { x: e.clientX || box.left + 16, y: e.clientY || box.bottom, node }
  }

  function closeMenu() {
    menu = null
  }

  function startCreate(node) {
    const dir = dirFor(node)
    prompt = { kind: 'create', dir, value: dir ? `${dir}/untitled.tex` : 'untitled.tex' }
    closeMenu()
  }

  function startRename(node) {
    prompt = { kind: 'rename', node, value: node.path }
    closeMenu()
  }

  async function submitPrompt(name) {
    const current = prompt
    prompt = null
    if (current.kind === 'create') await ops.create(name)
    else await ops.rename(current.node.path, name)
  }

  async function remove(node) {
    closeMenu()
    const count = node.kind === 'dir' ? ops && project.files().filter((f) => f.path.startsWith(`${node.path}/`)).length : 1
    const what = node.kind === 'dir' ? `the folder ${node.path} and its ${count} file${count === 1 ? '' : 's'}` : node.path
    if (!window.confirm(`Delete ${what}? Text files can be restored from History.`)) return
    await ops.remove(node)
  }

  function chooseFiles(e) {
    const files = [...e.currentTarget.files]
    e.currentTarget.value = ''
    if (files.length && ops) ops.upload(files, dirFor(menu?.node ?? null))
  }

  function onDrop(e, node) {
    e.preventDefault()
    dropTarget = null
    const files = [...(e.dataTransfer?.files ?? [])]
    if (files.length && ops) ops.upload(files, dirFor(node))
  }

  function onDragOver(e, node) {
    if (!ops || !e.dataTransfer?.types?.includes('Files')) return
    e.preventDefault()
    dropTarget = dirFor(node)
  }
</script>

<svelte:window onclick={closeMenu} onkeydown={(e) => e.key === 'Escape' && closeMenu()} />

<nav
  class="tree"
  class:dropping={dropTarget === ''}
  aria-label="Project files"
  ondragover={(e) => onDragOver(e, null)}
  ondragleave={() => (dropTarget = null)}
  ondrop={(e) => onDrop(e, null)}
  oncontextmenu={(e) => e.target === e.currentTarget && openMenu(e, null)}
>
  <header>
    <span>Files</span>
    <span class="tools">
      <button onclick={() => onview(view === 'type' ? 'folders' : 'type')} aria-pressed={view === 'type'} aria-label="Group files by type" title={view === 'type' ? 'Show folders' : 'Group by type'}>☷</button>
      {#if ops}
        <button onclick={() => startCreate(null)} aria-label="New file" title="New file">＋</button>
        <button onclick={() => { menu = null; picker?.click() }} aria-label="Upload files" title="Upload files">⇪</button>
      {/if}
      <button class="refresh" onclick={onrefresh} aria-label="Refresh file list" title="Refresh">↻</button>
    </span>
  </header>
  <input bind:this={picker} type="file" multiple hidden onchange={chooseFiles} />

  {#if project.error}
    <p class="note bad" role="alert">Could not load files: {project.error}</p>
  {:else if project.loading && rows.length === 0}
    <p class="note">Loading…</p>
  {:else if rows.length === 0}
    <p class="note">This folder has no files yet.</p>
  {/if}

  <ul role="tree">
    {#each rows as row (row.key)}
      {#if row.group}
        <li role="presentation" class="group">{row.group}</li>
      {:else}
      {@const node = row.node}
      {@const depth = row.depth}
      <li role="none">
        <button
          class="row"
          class:active={node.path === documents.activePath}
          class:muted={node.kind === 'file' && !openable(node)}
          class:drop={dropTarget !== null && node.kind === 'dir' && dropTarget === node.path}
          style:padding-left={`${0.5 + depth * 0.9}rem`}
          role="treeitem"
          aria-level={depth + 1}
          aria-expanded={node.kind === 'dir' ? project.expanded.has(node.path) : undefined}
          aria-selected={node.path === documents.activePath}
          disabled={node.kind === 'file' && !openable(node)}
          title={node.kind === 'file' && !openable(node) ? 'Not a text file' : node.path}
          onclick={() => activate(node)}
          oncontextmenu={(e) => { e.stopPropagation(); openMenu(e, node) }}
          onkeydown={(e) => (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) && openMenu(e, node)}
          ondragover={(e) => { e.stopPropagation(); onDragOver(e, node) }}
          ondrop={(e) => { e.stopPropagation(); onDrop(e, node) }}
        >
          <span class="glyph" aria-hidden="true">
            {node.kind === 'dir' ? (project.expanded.has(node.path) ? '▾' : '▸') : ''}
          </span>
          <span class="name">{node.name}{#if view === 'type' && node.path.includes('/')}<span class="dir"> {parentOf(node.path)}</span>{/if}</span>
          {#if node.path === (project.mainOverride ?? mainFile)}<span class="badge" title="Main document">main</span>{/if}
          {#if errorFiles.has(node.path)}<span class="err" role="img" aria-label="has errors" title="Has errors">●</span>{/if}
          {#if documents.get(node.path)?.dirty}<span class="dot" aria-label="unsaved">●</span>{/if}
        </button>
      </li>
      {/if}
    {/each}
  </ul>
</nav>

{#if menu}
  <ul class="menu" role="menu" style:left={`${menu.x}px`} style:top={`${menu.y}px`}>
    <li role="none"><button role="menuitem" onclick={() => startCreate(menu.node)}>New file…</button></li>
    <li role="none"><button role="menuitem" onclick={() => picker?.click()}>Upload files…</button></li>
    {#if menu.node}
      <li role="none"><button role="menuitem" onclick={() => startRename(menu.node)}>Rename…</button></li>
      {#if menu.node.kind === 'file' && menu.node.path.endsWith('.tex')}
        <li role="none">
          {#if project.mainOverride === menu.node.path}
            <button role="menuitem" onclick={() => (project.setMain(null), closeMenu())}>Stop using as main file</button>
          {:else}
            <button role="menuitem" onclick={() => (project.setMain(menu.node.path), closeMenu())}>Set as main file</button>
          {/if}
        </li>
      {/if}
      <li role="none"><button role="menuitem" class="danger" onclick={() => remove(menu.node)}>Delete…</button></li>
    {/if}
  </ul>
{/if}

{#if prompt}
  <NamePrompt
    title={prompt.kind === 'create' ? 'New file' : `Rename ${prompt.node.name}`}
    value={prompt.value}
    confirmLabel={prompt.kind === 'create' ? 'Create' : 'Rename'}
    onsubmit={submitPrompt}
    oncancel={() => (prompt = null)}
  />
{/if}

{#if previewPath}
  <ImagePreview path={previewPath} onclose={() => (previewPath = null)} />
{/if}

<style>
  .tree { display: flex; flex-direction: column; height: 100%; overflow: auto; background: hsl(var(--surface-1)); font-size: 13px; }
  header { display: flex; justify-content: space-between; align-items: center; padding: 0.5rem 0.75rem; font-weight: 600; color: hsl(var(--text-dim)); text-transform: uppercase; letter-spacing: 0.04em; font-size: 11px; }
  .tools { display: flex; gap: 0.15rem; }
  .tools button { background: none; border: none; cursor: pointer; color: hsl(var(--text-dim)); font-size: 14px; }
  .tools button:hover { color: hsl(var(--foreground)); }
  .tree.dropping { outline: 2px dashed hsl(var(--primary)); outline-offset: -2px; }
  .row.drop { background: hsl(var(--primary) / 0.2); }
  .menu { position: fixed; z-index: 800; min-width: 12rem; margin: 0; padding: 0.25rem 0; list-style: none; background: hsl(var(--popover)); color: hsl(var(--popover-foreground)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); box-shadow: 0 8px 24px rgb(0 0 0 / 0.3); font-size: 13px; }
  .menu button { display: block; width: 100%; text-align: left; padding: 0.35rem 0.8rem; background: none; border: none; font: inherit; color: inherit; cursor: pointer; }
  .menu button:hover { background: hsl(var(--accent)); }
  .menu .danger { color: hsl(var(--err)); }
  ul { list-style: none; margin: 0; padding: 0; }
  .row { display: flex; align-items: center; gap: 0.3rem; width: 100%; text-align: left; padding: 0.25rem 0.5rem; background: none; border: none; color: hsl(var(--foreground)); font: inherit; cursor: pointer; }
  .row:hover:not(:disabled) { background: hsl(var(--accent)); }
  .row.active { background: hsl(var(--accent)); font-weight: 600; }
  .row.muted { color: hsl(var(--text-faint)); cursor: default; }
  .glyph { width: 0.9rem; color: hsl(var(--text-faint)); flex-shrink: 0; }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .badge { margin-left: auto; font-size: 10px; padding: 0 0.35rem; border: 1px solid hsl(var(--border)); border-radius: var(--radius-pill); color: hsl(var(--text-dim)); }
  .group { padding: 0.5rem 0.75rem 0.15rem; font-size: 11px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.04em; color: hsl(var(--text-faint)); }
  .dir { color: hsl(var(--text-faint)); font-size: 11px; }
  .err { margin-left: auto; color: hsl(var(--err)); font-size: 9px; }
  .tools button[aria-pressed='true'] { color: hsl(var(--primary)); }
  .dot { margin-left: auto; color: hsl(var(--warn)); font-size: 9px; }
  .note { padding: 0.5rem 0.75rem; color: hsl(var(--text-dim)); }
  .note.bad { color: hsl(var(--err)); }
</style>
