<script>
  import { api } from '../lib/api/client.js'
  import Icon from './Icon.svelte'
  import { createEventDispatcher, onMount } from 'svelte'

  const dispatch = createEventDispatcher()

  let allFiles = []  // Accumulated files
  let tree = {}      // Derived tree structure for display
  let virtualFiles = []  // Flattened list for virtual scrolling
  let expanded = {}
  let selected = new Set()
  let searchQuery = ''
  let loading = false
  let offset = 0
  const limit = 100
  let hasMore = false
  let loadError = ''
  let treeContainer

  onMount(async () => {
    await loadFiles(true)
  })

  async function loadFiles(reset = false) {
    if (reset) {
      offset = 0
      allFiles = []
      hasMore = false
    }
    loading = true
    loadError = ''
    try {
      const params = { limit: String(limit), offset: String(offset) }
      if (searchQuery.trim()) {
        params.search = searchQuery.trim()
      }

      const data = await api.listFiles(params)
      const files = (data.files || []).map(toFileEntry)
      allFiles = reset ? files : [...allFiles, ...files]
      offset += files.length
      hasMore = Boolean(data.has_more)
      tree = organizeIntoTree(allFiles)
    } catch (e) {
      console.error('Failed to load files:', e)
      loadError = 'Failed to load files'
    } finally {
      loading = false
    }
  }

  /**
   * The server returns `files` as plain path strings (Vec<String> in
   * routes/files.rs), while the rest of this component wants `{ path, name }`.
   * Nothing bridged the two, so organizeIntoTree hit `file.path.substring` on
   * an undefined and every load ended in "Failed to load files" -- the tree has
   * never listed anything. Objects are passed through in case the endpoint ever
   * grows richer entries.
   */
  function toFileEntry(entry) {
    if (entry && typeof entry === 'object') return entry
    const path = String(entry)
    const slash = path.lastIndexOf('/')
    return { path, name: slash === -1 ? path : path.slice(slash + 1) }
  }

  function organizeIntoTree(fileList) {
    // Group files by directory
    const treeMap = {}
    fileList.forEach(file => {
      const dir = file.dir || file.path.substring(0, file.path.lastIndexOf('/')) || 'root'
      if (!treeMap[dir]) treeMap[dir] = []
      treeMap[dir].push(file)
    })
    return treeMap
  }
  
  // `tree` and `expanded` are arguments rather than closure reads on purpose.
  // Svelte works out a reactive statement's dependencies from the expression
  // itself, and `flattenTree()` mentions neither, so the statement ran once at
  // init -- while `tree` was still {} -- and never again. virtualFiles stayed
  // empty, the `{#if virtualFiles.length > 0}` guard never opened, and the tree
  // rendered nothing however many files loaded.
  function flattenTree(treeByDir, expandedDirs) {
    // Each item is either a directory header or a file under it.
    const items = []
    Object.entries(treeByDir).forEach(([dir, dirFiles]) => {
      items.push({ type: 'dir', dir, fileCount: dirFiles.length })
      if (expandedDirs[dir]) {
        dirFiles.forEach((file) => {
          items.push({ type: 'file', file })
        })
      }
    })
    return items
  }

  $: virtualFiles = flattenTree(tree, expanded)

  function toggleExpand(path) {
    expanded[path] = !expanded[path]
  }

  function toggleSelect(path) {
    if (selected.has(path)) {
      selected.delete(path)
    } else {
      selected.add(path)
    }
    selected = selected // Trigger reactivity
    dispatch('selection-changed', Array.from(selected))
  }

  function selectAll() {
    allFiles.forEach(file => {
      selected.add(file.path)
    })
    selected = selected
    dispatch('selection-changed', Array.from(selected))
  }

  function clearSelection() {
    selected.clear()
    selected = selected
    dispatch('selection-changed', [])
  }

  $: filteredTree = tree
  $: selectedCount = selected.size

  async function handleSearchChange(value) {
    searchQuery = value
    await loadFiles(true)
  }

  async function handleLoadMore() {
    if (loading || !hasMore) return
    await loadFiles(false)
  }

  function handleScroll(e) {
    const target = e.currentTarget
    if (!hasMore || loading) return
    const nearBottom = target.scrollTop + target.clientHeight >= target.scrollHeight - 60
    if (nearBottom) {
      handleLoadMore()
    }
  }
  
  function getDirFiles(dirName) {
    return tree[dirName] || []
  }

  function formatSize(bytes) {
    if (!bytes) return ''
    if (bytes < 1024) return `${bytes}B`
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`
    return `${(bytes / (1024 * 1024)).toFixed(1)}MB`
  }
</script>

<div class="file-tree">
  <div class="file-tree-header">
    <h3 id="file-tree-title">Files ({Object.keys(filteredTree).length} directories)</h3>
    <div class="search-box">
      <label for="file-search" class="sr-only">Search files</label>
      <input
        id="file-search"
        type="text"
        placeholder="Search files..."
        bind:value={searchQuery}
        on:change={(e) => handleSearchChange(e.target.value)}
        class="search-input"
        aria-label="Search files by name or path"
      />
    </div>
  </div>

  {#if loading}
    <div class="loading" role="status" aria-live="polite">Loading files...</div>
  {:else if loadError}
    <div class="no-files" role="status" aria-live="polite">{loadError}</div>
  {:else if Object.keys(filteredTree).length === 0}
    <div class="no-files" role="status" aria-live="polite">No .tex files found</div>
  {:else}
    <div class="controls" role="toolbar" aria-label="File selection controls">
      {#if selectedCount > 0}
        <span class="selected-count" role="status" aria-live="polite" aria-atomic="true">
          {selectedCount} {selectedCount === 1 ? 'file' : 'files'} selected
        </span>
        <button 
          on:click={clearSelection} 
          class="btn-clear"
          aria-label={`Deselect all ${selectedCount} selected files`}
        >
          Clear
        </button>
      {:else}
        <button 
          on:click={selectAll} 
          class="btn-select-all"
          aria-label={`Select all ${allFiles.length} files`}
        >
          Select All
        </button>
      {/if}
    </div>

    <div 
      class="tree-items"
      bind:this={treeContainer}
      on:scroll={handleScroll}
      role="tree"
      aria-labelledby="file-tree-title"
      aria-multiselectable="true"
    >
      <!-- A plain scrolling list. svelte-virtual-list is a Svelte 3 package --
           the build already warns it has no `svelte` exports condition -- and
           under Svelte 5 it rendered its viewport with no height, so it
           computed zero visible rows and showed nothing however many files
           loaded. The list is paginated by handleScroll anyway, so
           virtualising it was buying nothing for a workspace of this size. -->
      {#if virtualFiles.length > 0}
        {#each virtualFiles as item (item.type === 'dir' ? `d:${item.dir}` : `f:${item.file.path}`)}
          {#if item.type === 'dir'}
            <button
              on:click={() => toggleExpand(item.dir)}
              class="dir-toggle"
              role="treeitem"
              aria-expanded={!!expanded[item.dir]}
              aria-selected="false"
              aria-label={`${expanded[item.dir] ? 'Collapse' : 'Expand'} directory: ${item.dir === 'root' ? 'root' : item.dir} (${item.fileCount} files)`}
            >
              <Icon name={expanded[item.dir] ? 'folder-open' : 'folder'} /> {item.dir === 'root' ? 'root' : item.dir}
            </button>
          {:else if item.type === 'file'}
            <div class="tree-item" role="none">
              <input
                type="checkbox"
                checked={selected.has(item.file.path)}
                on:change={() => toggleSelect(item.file.path)}
                class="file-checkbox"
                aria-label={`Select file ${item.file.name}`}
                title={`Select ${item.file.name}`}
              />
              <button 
                class="file-name-btn" 
                type="button"
                on:click={() => dispatch('file-selected', item.file.path)}
                role="treeitem"
                aria-selected={selected.has(item.file.path)}
                aria-label={`Open file ${item.file.name} (${formatSize(item.file.size)})`}
              >
                <Icon name="document" /> {item.file.name}
              </button>
              <span class="file-size" aria-label={`File size: ${formatSize(item.file.size)}`}>{formatSize(item.file.size)}</span>
            </div>
          {/if}
        {/each}
      {/if}
    </div>

    {#if hasMore}
      <div class="load-more">
        <button class="btn-load-more" on:click={handleLoadMore} disabled={loading}>
          {loading ? 'Loading...' : 'Load more files'}
        </button>
      </div>
    {/if}
  {/if}
</div>

<style>
  .file-tree {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: hsl(var(--surface-2));
    border-right: 1px solid hsl(var(--border));
  }

  .file-tree-header {
    padding: 1rem;
    background: hsl(var(--surface-1));
    border-bottom: 1px solid hsl(var(--border));
  }

  .file-tree-header h3 {
    margin: 0 0 0.75rem 0;
    font-size: 14px;
  }

  .search-box {
    margin-bottom: 0.5rem;
  }

  .search-input {
    width: 100%;
    padding: 0.4rem;
    border: 1px solid hsl(var(--border));
    border-radius: var(--radius-xs);
    font-size: 12px;
  }

  .search-input:focus {
    outline: none;
    border-color: hsl(var(--primary));
  }

  .loading {
    padding: 2rem 1rem;
    text-align: center;
    color: hsl(var(--text-dim));
    font-size: 12px;
  }

  .no-files {
    padding: 2rem 1rem;
    text-align: center;
    color: hsl(var(--text-faint));
    font-size: 12px;
  }

  .controls {
    padding: 0.5rem 1rem;
    background: hsl(var(--surface-2));
    border-bottom: 1px solid hsl(var(--border));
    display: flex;
    gap: 0.5rem;
    align-items: center;
    font-size: 12px;
  }

  .selected-count {
    color: hsl(var(--primary));
    font-weight: bold;
  }

  .btn-select-all,
  .btn-clear {
    padding: 0.3rem 0.6rem;
    background: hsl(var(--primary));
    color: hsl(var(--primary-foreground));
    border: none;
    border-radius: var(--radius-xs);
    cursor: pointer;
    font-size: 11px;
  }

  .btn-clear {
    background: hsl(var(--err));
  }

  .btn-select-all:hover,
  .btn-clear:hover {
    opacity: 0.8;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border-width: 0;
  }

  .tree-items {
    flex: 1;
    overflow-y: auto;
  }

  .load-more {
    padding: 0.75rem 1rem;
    border-top: 1px solid hsl(var(--border));
    background: hsl(var(--surface-2));
  }

  .btn-load-more {
    width: 100%;
    padding: 0.5rem 0.75rem;
    background: hsl(var(--primary));
    color: hsl(var(--primary-foreground));
    border: none;
    border-radius: var(--radius-xs);
    cursor: pointer;
    font-size: 12px;
  }

  .btn-load-more:disabled {
    background: hsl(var(--accent));
    cursor: not-allowed;
  }

  .directory-group {
    border-bottom: 1px solid hsl(var(--border));
  }

  .dir-toggle {
    width: 100%;
    padding: 0.6rem 1rem;
    text-align: left;
    background: hsl(var(--surface-2));
    border: none;
    border-bottom: 1px solid hsl(var(--border));
    cursor: pointer;
    font-size: 13px;
    font-weight: bold;
  }

  .dir-toggle:hover {
    background: hsl(var(--muted));
  }

  .dir-contents {
    padding: 0.5rem 0;
  }

  .tree-item {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.4rem 1.5rem 0.4rem 2rem;
    font-size: 12px;
  }

  .file-checkbox {
    cursor: pointer;
  }

  .file-name-btn {
    flex: 1;
    cursor: pointer;
    color: hsl(var(--primary));
    background: transparent;
    border: none;
    padding: 0;
    text-align: left;
  }

  .file-name-btn:hover {
    text-decoration: underline;
  }

  .file-size {
    color: hsl(var(--text-faint));
    font-size: 11px;
  }
</style>
