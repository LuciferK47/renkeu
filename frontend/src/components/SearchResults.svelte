<script>
  import { createEventDispatcher } from 'svelte'
  import { searchWorkspace } from '../services/fileService.js'
  
  const dispatch = createEventDispatcher()
  
  export let searchQuery = ''
  export let results = []
  export let loading = false
  export let visible = false
  
  let caseSensitive = false
  let filePattern = '*.tex'
  let searchInput = ''
  let searchTimer = null
  
  async function performSearch() {
    if (!searchQuery.trim()) {
      results = []
      return
    }
    
    loading = true
    
    try {
      // Backend nucleo fuzzy search over workspace files -> { hits: [{value, score}] }
      const hits = await searchWorkspace(searchQuery)
      results = hits.map(hit => ({ file: hit.value, score: hit.score }))
    } catch (err) {
      console.error('Search error:', err)
      results = []
    } finally {
      loading = false
    }
  }
  
  function handleInput(e) {
    searchQuery = e.target.value
    
    // Debounce search
    if (searchTimer) clearTimeout(searchTimer)
    
    if (searchQuery.trim()) {
      searchTimer = setTimeout(() => {
        performSearch()
      }, 300)
    } else {
      results = []
    }
  }
  
  function openResult(file, lineNumber) {
    dispatch('open', { file, lineNumber })
  }
  
  function highlightMatch(text, query) {
    if (!query) return text
    
    try {
      const regex = new RegExp(`(${query})`, caseSensitive ? 'g' : 'gi')
      return text.replace(regex, '<mark>$1</mark>')
    } catch (e) {
      return text
    }
  }
  
  function close() {
    visible = false
    dispatch('close')
  }
</script>

{#if visible}
  <div class="search-panel" role="search" aria-label="Search results">
    <div class="search-header">
      <h3>Search Workspace</h3>
      <button
        class="close-btn"
        on:click={close}
        aria-label="Close search"
      >
        [X]
      </button>
    </div>
    
    <div class="search-controls">
      <input
        type="text"
        class="search-input"
        placeholder="Search query (supports regex)..."
        bind:value={searchQuery}
        on:input={handleInput}
        aria-label="Search query"
      />
      
      <div class="search-options">
        <label class="checkbox-label">
          <input type="checkbox" bind:checked={caseSensitive} on:change={performSearch} />
          Case sensitive
        </label>
        
        <input
          type="text"
          class="pattern-input"
          placeholder="File pattern"
          bind:value={filePattern}
          on:change={performSearch}
          aria-label="File pattern"
        />
      </div>
    </div>
    
    {#if loading}
      <div class="loading" role="status" aria-live="polite">
        Searching...
      </div>
    {:else if results.length === 0 && searchQuery}
      <div class="no-results" role="status" aria-live="polite">
        No results found for "{searchQuery}"
      </div>
    {:else if results.length > 0}
      <div class="results-container" role="list">
        <div class="results-count" role="status" aria-live="polite">
          Found {results.length} file{results.length !== 1 ? 's' : ''}
        </div>
        
        {#each results as result}
          <div class="result-file" role="listitem">
            <div class="file-name">
              <strong>{result.file}</strong>
              <span class="match-count">({result.matches.length} match{result.matches.length !== 1 ? 'es' : ''})</span>
            </div>
            
            <div class="matches">
              {#each result.matches as match}
                <button
                  class="match-line"
                  on:click={() => openResult(result.file, match.line_number)}
                  aria-label="Jump to line {match.line_number}"
                >
                  <span class="line-number">{match.line_number}</span>
                  <span class="line-content">{@html highlightMatch(match.line_content, searchQuery)}</span>
                </button>
              {/each}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .search-panel {
    position: fixed;
    top: 60px;
    right: 20px;
    width: 400px;
    max-height: 600px;
    background: white;
    border: 1px solid #ddd;
    border-radius: 8px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
    display: flex;
    flex-direction: column;
    z-index: 1000;
  }
  
  .search-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 1rem;
    border-bottom: 1px solid #eee;
  }
  
  .search-header h3 {
    margin: 0;
    font-size: 1.1rem;
    color: #333;
  }
  
  .close-btn {
    background: none;
    border: none;
    font-size: 1.5rem;
    cursor: pointer;
    color: #666;
    padding: 0;
    width: 24px;
    height: 24px;
    line-height: 1;
  }
  
  .close-btn:hover {
    color: #000;
  }
  
  .search-controls {
    padding: 1rem;
    border-bottom: 1px solid #eee;
  }
  
  .search-input {
    width: 100%;
    padding: 0.5rem;
    border: 1px solid #ccc;
    border-radius: 4px;
    font-size: 0.95rem;
    margin-bottom: 0.75rem;
  }
  
  .search-options {
    display: flex;
    gap: 1rem;
    align-items: center;
  }
  
  .checkbox-label {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    font-size: 0.9rem;
    cursor: pointer;
  }
  
  .pattern-input {
    flex: 1;
    padding: 0.4rem;
    border: 1px solid #ccc;
    border-radius: 4px;
    font-size: 0.85rem;
  }
  
  .results-container {
    flex: 1;
    overflow-y: auto;
    padding: 0.5rem;
  }
  
  .results-count {
    padding: 0.5rem 1rem;
    font-size: 0.9rem;
    color: #666;
    border-bottom: 1px solid #eee;
  }
  
  .result-file {
    margin-bottom: 1rem;
    border-bottom: 1px solid #f0f0f0;
    padding-bottom: 0.5rem;
  }
  
  .file-name {
    padding: 0.5rem 1rem;
    background: #f8f8f8;
    border-radius: 4px;
    margin-bottom: 0.5rem;
  }
  
  .file-name strong {
    color: #333;
    font-size: 0.95rem;
  }
  
  .match-count {
    margin-left: 0.5rem;
    color: #888;
    font-size: 0.85rem;
  }
  
  .matches {
    display: flex;
    flex-direction: column;
  }
  
  .match-line {
    display: flex;
    align-items: flex-start;
    padding: 0.4rem 0.75rem;
    border: none;
    background: none;
    cursor: pointer;
    text-align: left;
    border-radius: 4px;
    transition: background 0.2s;
  }
  
  .match-line:hover {
    background: #f0f7ff;
  }
  
  .line-number {
    min-width: 40px;
    color: #0066cc;
    font-family: monospace;
    font-size: 0.85rem;
    flex-shrink: 0;
  }
  
  .line-content {
    color: #555;
    font-family: monospace;
    font-size: 0.85rem;
    line-height: 1.4;
    word-break: break-all;
  }
  
  .line-content :global(mark) {
    background: #ffeb3b;
    padding: 0 2px;
    border-radius: 2px;
  }
  
  .loading,
  .no-results {
    padding: 2rem;
    text-align: center;
    color: #666;
    font-size: 0.95rem;
  }
  
  /* Dark mode support */
  @media (prefers-color-scheme: dark) {
    .search-panel {
      background: #2d2d2d;
      border-color: #444;
    }
    
    .search-header h3 {
      color: #eee;
    }
    
    .close-btn {
      color: #aaa;
    }
    
    .close-btn:hover {
      color: #fff;
    }
    
    .search-input,
    .pattern-input {
      background: #1e1e1e;
      color: #eee;
      border-color: #555;
    }
    
    .file-name {
      background: #333;
    }
    
    .file-name strong {
      color: #eee;
    }
    
    .match-line:hover {
      background: #3a3a3a;
    }
    
    .line-content {
      color: #ccc;
    }
  }
</style>
