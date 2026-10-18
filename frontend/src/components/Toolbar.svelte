<script>
  import { formatChord } from '../lib/commands.js'

  let {
    ui,
    documents,
    compile,
    project,
    commands,
    oncompile = () => {},
    oncancel = () => {},
    onsettings = () => {},
    onhistory = () => {},
    onpalette = () => {},
    onopenproject = null
  } = $props()

  const shortcut = (id) => {
    const keys = commands.get(id)?.keys
    return keys ? ` (${formatChord(keys)})` : ''
  }

  // Offer a root-document choice when there is a real choice to make: the
  // project could not decide, or the user already decided.
  const roots = $derived(
    compile.candidates.length > 0
      ? compile.candidates
      : project.editableFiles().map((f) => f.path).filter((p) => p.endsWith('.tex'))
  )
  const showMain = $derived(compile.candidates.length > 0 || project.mainOverride !== null)

  const nextTheme = { auto: 'light', light: 'dark', dark: 'auto' }
</script>

<header class="toolbar" aria-label="Toolbar">
  <button class="text" aria-pressed={ui.sidebar} title={`Files${shortcut('toggle-sidebar')}`} onclick={() => ui.toggle('sidebar')}>Files</button>

  {#if onopenproject}
    <button class="text project" onclick={onopenproject} title={project.root ? `${project.root}\nOpen another folder` : 'Open a project folder'}>
      {project.name || 'Open folder…'} ▾
    </button>
  {/if}

  <span class="file" title={documents.active?.path ?? ''}>
    {documents.active?.path ?? 'LaTeX-AI'}
  </span>

  {#if showMain}
    <label class="main">
      <span>Main</span>
      <select
        value={project.mainOverride ?? ''}
        onchange={(e) => project.setMain(e.currentTarget.value || null)}
        aria-label="Main document"
      >
        <option value="">Auto-detect</option>
        {#each roots as path (path)}<option value={path}>{path}</option>{/each}
      </select>
    </label>
  {/if}

  <span class="spacer"></span>

  {#if compile.compiling}
    <button class="stop" onclick={oncancel} title="Stop the compile">Stop</button>
  {/if}
  <button
    class="primary"
    onclick={oncompile}
    disabled={!documents.active || compile.compiling}
    aria-busy={compile.compiling}
    title={`Compile to PDF${shortcut('compile')}`}
  >{compile.compiling ? 'Compiling…' : 'Compile'}</button>

  <button class="text" onclick={onpalette} title={`Ask the AI${shortcut('palette')}`}>Ask AI</button>
  <button class="text" aria-pressed={ui.problems} onclick={() => ui.toggle('problems')} title={`Problems${shortcut('toggle-problems')}`}>
    Problems{#if compile.errors.length > 0}<span class="n">{compile.errors.length}</span>{/if}
  </button>
  <button class="text" aria-pressed={ui.chat} onclick={() => ui.toggle('chat')} title={`Assistant${shortcut('toggle-chat')}`}>Assistant</button>
  <button class="text" onclick={onhistory} title="History">History</button>
  <button class="text" onclick={() => ui.setTheme(nextTheme[ui.theme])} title="Switch theme">Theme: {ui.theme}</button>
  <button class="text" onclick={onsettings} title="Settings">Settings</button>
</header>

<style>
  .project { max-width: 14rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 600; }
  .toolbar { display: flex; align-items: center; gap: 0.4rem; padding: 0.35rem 0.6rem; background: hsl(var(--surface-1)); border-bottom: 1px solid hsl(var(--border)); }
  .file { font-weight: 600; font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 32ch; }
  .spacer { flex: 1; }
  button { font: inherit; color: hsl(var(--foreground)); cursor: pointer; border-radius: var(--radius-xs); }
  .text { background: none; border: 1px solid transparent; padding: 0.2rem 0.6rem; font-size: 13px; color: hsl(var(--text-dim)); }
  .text:hover { background: hsl(var(--accent)); color: hsl(var(--foreground)); }
  .text[aria-pressed='true'] { background: hsl(var(--accent)); border-color: hsl(var(--border)); color: hsl(var(--foreground)); }
  .n { margin-left: 0.3rem; padding: 0 0.35rem; border-radius: var(--radius-pill); background: hsl(var(--err)); color: hsl(var(--destructive-foreground)); font-size: 11px; font-weight: 700; }
  .primary { background: hsl(var(--primary)); color: hsl(var(--primary-foreground)); border: none; padding: 0.3rem 1rem; }
  .primary:disabled { opacity: 0.45; cursor: default; }
  .stop { background: none; border: 1px solid hsl(var(--err)); color: hsl(var(--err)); padding: 0.25rem 0.7rem; }
  .main { display: flex; align-items: center; gap: 0.3rem; font-size: 12px; color: hsl(var(--text-dim)); }
  .main select { background: hsl(var(--surface-0)); color: hsl(var(--foreground)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); font: inherit; padding: 0.1rem 0.3rem; max-width: 16ch; }
</style>
