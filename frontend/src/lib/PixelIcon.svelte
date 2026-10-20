<script>
  let { name = 'dot', size = 16, label = '' } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 16 16"
  role={label ? 'img' : undefined}
  aria-label={label || undefined}
  aria-hidden={label ? undefined : 'true'}
  class="pixel-icon"
>
  {#if name === 'arrow'}
    <path fill="currentColor" d="M1 7h9V4l5 4-5 4V9H1z" />
  {:else if name === 'github'}
    <path fill="currentColor" d="M5 1h6v1h2v2h1v7h-2v2h-2v2H6v-2H4v-2H2V4h1V2h2zm1 2H5v2H4v5h2v2h1v1h2v-2h1V10h2V5h-1V3zm1 2h2v1H7zm-2 2h1v2H5zm5 0h1v2h-1z" />
  {:else if name === 'sun'}
    <path fill="currentColor" d="M7 0h2v3H7zm0 13h2v3H7zM0 7h3v2H0zm13 0h3v2h-3zM2 2l2 2-1 1-2-2zm11 9 2 2-1 1-2-2zM2 14l2-2 1 1-2 2zm10-10 2-2 1 1-2 2zM5 5h6v6H5z" />
  {:else if name === 'moon'}
    <path fill="currentColor" d="M11 1a6 6 0 1 0 4 10A6 6 0 0 1 11 1z" />
  {:else if name === 'copy'}
    <path fill="currentColor" d="M4 1h9v10h-2V3H4zm-2 4h8v10H2zm2 2v6h4V7z" />
  {:else if name === 'download'}
    <path fill="currentColor" d="M7 1h2v7h2l-3 3-3-3h2zM2 13h12v2H2z" />
  {:else if name === 'branch'}
    <path fill="currentColor" d="M4 1a2 2 0 1 0 1 3.73V6h5v2.27A2 2 0 1 0 11 12V7a2 2 0 0 0-2-2H5V4.73A2 2 0 0 0 4 1zm0 2a.5.5 0 1 1 0 1 .5.5 0 0 1 0-1zm7 7a.5.5 0 1 1 0 1 .5.5 0 0 1 0-1z" />
  {:else if name === 'lock'}
    <path fill="currentColor" d="M4 6V4a4 4 0 0 1 8 0v2h2v9H2V6zm2 0h4V4a2 2 0 0 0-4 0zm1 3v3h2V9z" />
  {:else if name === 'terminal'}
    <path fill="currentColor" d="M1 2h14v12H1zm2 2v2l2 2-2 2v2l4-4zm5 6h4v2H8z" />
  {:else if name === 'spark'}
    <path fill="currentColor" d="M7 0h2v5h5v2H9v5H7V7H2V5h5zM13 11h1v2h2v1h-2v2h-1v-2h-2v-1h2z" />
  {:else if name === 'book'}
    <path fill="currentColor" d="M2 2h5a2 2 0 0 1 2 2v10a2 2 0 0 0-2-1H2zm12 0H9v11a2 2 0 0 1 2-1h3z" />
  {:else if name === 'search'}
    <path fill="currentColor" d="M6 1a5 5 0 1 0 3 9l4 4 1-1-4-4a5 5 0 0 0-4-8zm0 2a3 3 0 1 1 0 6 3 3 0 0 1 0-6z" />
  {:else if name === 'history'}
    <path fill="currentColor" d="M2 7a6 6 0 1 1 2 6l-1 1-1-1 1-1a4 4 0 1 0-1-5H5v2H1V5h2zM7 5h2v3h2v2H7z" />
  {:else if name === 'play'}
    <path fill="currentColor" d="M4 2v12l10-6z" />
  {:else if name === 'gear'}
    <path fill="currentColor" fill-rule="evenodd" d="M6 1h4v2h2v2h2v4h-2v2h-2v2H6v-2H4v-2H2V5h2V3h2zm1 5v4h2V6z" />
  {:else if name === 'warn'}
    <path fill="currentColor" fill-rule="evenodd" d="M7 1h2v2h1v2h1v2h1v2h1v2h1v3H1v-3h1v-2h1V7h1V5h1V3h1zm0 5v4h2V6zm0 5v2h2v-2z" />
  {:else if name === 'bot'}
    <path fill="currentColor" fill-rule="evenodd" d="M7 0h2v2h4v2h1v7h-1v2H3v-2H2V4h1V2h4zM4 4v7h8V4zm1 2h2v2H5zm4 0h2v2H9zM1 6h1v3H1zm13 0h1v3h-1z" />
  {:else if name === 'fit'}
    <path fill="currentColor" d="M1 3h2v10H1zm12 0h2v10h-2zM5 7h6V5l3 3-3 3V9H5v2L2 8l3-3z" />
  {:else if name === 'pause'}
    <path fill="currentColor" d="M4 3h3v10H4zm5 0h3v10H9z" />
  {:else if name === 'crab'}
    <path fill="currentColor" fill-rule="evenodd" d="M4 1h2v3H4zm6 0h2v3h-2zM0 3h3v4H0zm13 0h3v4h-3zM3 4h10v7H3zM3 11h2v4H3zm4 0h2v4H7zm4 0h2v4h-2zM5 6h2v2H5zm4 0h2v2H9z" />
  {:else if name === 'target'}
    <path fill="currentColor" d="M7 1h2v4H7zm0 10h2v4H7zM1 7h4v2H1zm10 0h4v2h-4zM7 7h2v2H7z" />
  {:else if name === 'select'}
    <path fill="currentColor" d="M2 2h4v2H2zm8 0h4v2h-4zM2 12h4v2H2zm8 0h4v2h-4zM2 4h2v2H2zm10 0h2v2h-2zM2 10h2v2H2zm10 0h2v2h-2z" />
  {:else if name === 'folder'}
    <path fill="currentColor" d="M1 3h5l2 2h7v9H1z" />
  {:else if name === 'tex'}
    <path fill="currentColor" fill-rule="evenodd" d="M2 2h12v12H2zm2 2v2h3v6h2V6h3V4z" />
  {:else}
    <rect x="5" y="5" width="6" height="6" fill="currentColor" />
  {/if}
</svg>

<style>
  .pixel-icon { display: block; flex: 0 0 auto; shape-rendering: crispEdges; }
</style>
