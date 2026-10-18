<script>
  // The project tree. Directories expand and collapse; clicking an editable
  // file opens it. Non-text files (figures, PDFs) are listed so the layout is
  // honest, but are not openable until the file manager (M13).
  let { project, documents, onopen = () => {}, onrefresh = () => {} } = $props()

  // Flatten to the rows that are visible given what is expanded.
  const rows = $derived.by(() => {
    const out = []
    const walk = (nodes, depth) => {
      for (const node of nodes) {
        out.push({ node, depth })
        if (node.kind === 'dir' && project.expanded.has(node.path)) walk(node.children ?? [], depth + 1)
      }
    }
    walk(project.tree, 0)
    return out
  })

  function activate(node) {
    if (node.kind === 'dir') project.toggle(node.path)
    else if (node.editable) onopen(node.path)
  }
</script>

<nav class="tree" aria-label="Project files">
  <header>
    <span>Files</span>
    <button class="refresh" onclick={onrefresh} aria-label="Refresh file list" title="Refresh">↻</button>
  </header>

  {#if project.error}
    <p class="note bad" role="alert">Could not load files: {project.error}</p>
  {:else if project.loading && rows.length === 0}
    <p class="note">Loading…</p>
  {:else if rows.length === 0}
    <p class="note">This folder has no files yet.</p>
  {/if}

  <ul role="tree">
    {#each rows as { node, depth } (node.path)}
      <li role="none">
        <button
          class="row"
          class:active={node.path === documents.activePath}
          class:muted={node.kind === 'file' && !node.editable}
          style:padding-left={`${0.5 + depth * 0.9}rem`}
          role="treeitem"
          aria-level={depth + 1}
          aria-expanded={node.kind === 'dir' ? project.expanded.has(node.path) : undefined}
          aria-selected={node.path === documents.activePath}
          disabled={node.kind === 'file' && !node.editable}
          title={node.kind === 'file' && !node.editable ? 'Not a text file' : node.path}
          onclick={() => activate(node)}
        >
          <span class="glyph" aria-hidden="true">
            {node.kind === 'dir' ? (project.expanded.has(node.path) ? '▾' : '▸') : ''}
          </span>
          <span class="name">{node.name}</span>
          {#if node.path === project.mainOverride}<span class="badge" title="Main document">main</span>{/if}
          {#if documents.get(node.path)?.dirty}<span class="dot" aria-label="unsaved">●</span>{/if}
        </button>
      </li>
    {/each}
  </ul>
</nav>

<style>
  .tree { display: flex; flex-direction: column; height: 100%; overflow: auto; background: hsl(var(--surface-1)); font-size: 13px; }
  header { display: flex; justify-content: space-between; align-items: center; padding: 0.5rem 0.75rem; font-weight: 600; color: hsl(var(--text-dim)); text-transform: uppercase; letter-spacing: 0.04em; font-size: 11px; }
  .refresh { background: none; border: none; cursor: pointer; color: hsl(var(--text-dim)); font-size: 14px; }
  ul { list-style: none; margin: 0; padding: 0; }
  .row { display: flex; align-items: center; gap: 0.3rem; width: 100%; text-align: left; padding: 0.25rem 0.5rem; background: none; border: none; color: hsl(var(--foreground)); font: inherit; cursor: pointer; }
  .row:hover:not(:disabled) { background: hsl(var(--accent)); }
  .row.active { background: hsl(var(--accent)); font-weight: 600; }
  .row.muted { color: hsl(var(--text-faint)); cursor: default; }
  .glyph { width: 0.9rem; color: hsl(var(--text-faint)); flex-shrink: 0; }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .badge { margin-left: auto; font-size: 10px; padding: 0 0.35rem; border: 1px solid hsl(var(--border)); border-radius: var(--radius-pill); color: hsl(var(--text-dim)); }
  .dot { margin-left: auto; color: hsl(var(--warn)); font-size: 9px; }
  .note { padding: 0.5rem 0.75rem; color: hsl(var(--text-dim)); }
  .note.bad { color: hsl(var(--err)); }
</style>
