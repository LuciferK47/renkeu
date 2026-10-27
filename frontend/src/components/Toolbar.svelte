<script>
  import { formatChord } from '../lib/commands.js'
  import PixelIcon from '../lib/PixelIcon.svelte'

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
    oninsert = () => {},
    onopenproject = null,
    onhome = null
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
  {#if onhome}
    <button class="brand-nav" onclick={onhome} title="Back to the Renkeu page">
      <PixelIcon name="crab" size={18} />
      <span class="brand-text">Ren<span class="ai">keu</span></span>
    </button>
  {/if}

  <button class="text" aria-pressed={ui.sidebar} title={`Files${shortcut('toggle-sidebar')}`} onclick={() => ui.toggle('sidebar')}>
    <PixelIcon name="folder" size={14} /> Files
  </button>

  {#if onopenproject}
    <button class="text project" onclick={onopenproject} title={project.root ? `${project.root}\nOpen another folder` : 'Open a project folder'}>
      {project.name || 'Open folder…'} ▾
    </button>
  {/if}

  <span class="file" title={documents.active?.path ?? ''}>
    {documents.active?.path ?? 'Renkeu'}
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
  ><PixelIcon name="play" size={13} />{compile.compiling ? 'Compiling…' : 'Compile'}</button>

  <span class="sep" aria-hidden="true"></span>
  <button class="text" onclick={oninsert} disabled={!documents.active} title={`Insert a figure, table, equation, citation or reference${shortcut('insert')}`}><PixelIcon name="select" size={14} /> Insert</button>
  <button class="text" onclick={onpalette} title={`Ask the AI${shortcut('palette')}`}><PixelIcon name="spark" size={14} /> Ask AI</button>
  <button class="text" aria-pressed={ui.problems} onclick={() => ui.toggle('problems')} title={`Problems${shortcut('toggle-problems')}`}>
    <PixelIcon name="warn" size={14} /> Problems{#if compile.errors.length > 0}<span class="n">{compile.errors.length}</span>{/if}
  </button>
  <button class="text" aria-pressed={ui.chat} onclick={() => ui.toggle('chat')} title={`Assistant${shortcut('toggle-chat')}`}><PixelIcon name="bot" size={14} /> Assistant</button>
  <span class="sep" aria-hidden="true"></span>
  <button class="text" onclick={onhistory} title="History"><PixelIcon name="history" size={14} /> History</button>
  <button class="text icon" onclick={() => ui.setTheme(nextTheme[ui.theme])} title={`Switch theme (now ${ui.theme})`} aria-label={`Colour mode: ${ui.theme}`}>
    <PixelIcon name={ui.theme === 'dark' ? 'moon' : ui.theme === 'light' ? 'sun' : 'gear'} size={14} />
    <span class="theme-word">{ui.theme}</span>
  </button>
  <button class="text" onclick={onsettings} title="Settings"><PixelIcon name="gear" size={14} /> Settings</button>

  <!-- Compile progress: a row of blocks that fills while the engine works. -->
  {#if compile.compiling}<div class="progress" role="progressbar" aria-label="Compiling" aria-busy="true"><i></i></div>{/if}
</header>

<style>
  .toolbar { position: relative; display: flex; align-items: center; gap: 0.35rem; padding: 0.35rem 0.6rem; background: hsl(var(--surface-1)); border-bottom: 1px solid hsl(var(--border)); }
  .file { font-weight: 600; font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 32ch; padding-left: 0.4rem; }
  .spacer { flex: 1; }
  .sep { width: 1px; align-self: stretch; margin: 0.15rem 0.2rem; background: hsl(var(--border)); }
  button { font: inherit; color: hsl(var(--foreground)); cursor: pointer; border-radius: var(--radius-xs); display: inline-flex; align-items: center; gap: 0.4rem; }
  .text { background: none; border: 1px solid transparent; padding: 0.2rem 0.6rem; font-size: 13px; color: hsl(var(--text-dim)); }
  .text:hover { background: hsl(var(--accent)); color: hsl(var(--foreground)); }
  .text[aria-pressed='true'] { background: hsl(var(--accent)); border-color: hsl(var(--border)); color: hsl(var(--foreground)); }
  .project { max-width: 14rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 600; }
  .icon .theme-word { font-size: 12px; text-transform: capitalize; }
  .n { margin-left: 0.2rem; padding: 0 0.35rem; border-radius: var(--radius-pill); background: hsl(var(--err)); color: hsl(var(--destructive-foreground)); font-size: 11px; font-weight: 700; }
  /* The one primary action. Dark ink on rust (5.6:1 in the dark theme), with a
   * hard pixel shadow instead of a soft one. */
  .primary { background: hsl(var(--primary)); color: hsl(var(--primary-foreground)); border: none; padding: 0.32rem 1rem; font-weight: 600; box-shadow: 0 2px 0 hsl(var(--primary) / 0.45); }
  .primary:active:not(:disabled) { transform: translateY(2px); box-shadow: none; }
  .primary:disabled { opacity: 0.45; cursor: default; box-shadow: none; }
  .stop { background: none; border: 1px solid hsl(var(--err)); color: hsl(var(--err)); padding: 0.25rem 0.7rem; }
  .main { display: flex; align-items: center; gap: 0.3rem; font-size: 12px; color: hsl(var(--text-dim)); }
  .main select { background: hsl(var(--surface-0)); color: hsl(var(--foreground)); border: 1px solid hsl(var(--border)); border-radius: var(--radius-xs); font: inherit; padding: 0.1rem 0.3rem; max-width: 16ch; }
  .brand-nav { background: none; border: 1px solid hsl(var(--border)); padding: 0.15rem 0.55rem; font-weight: 700; letter-spacing: 0.01em; font-size: 14px; color: hsl(var(--primary)); margin-right: 0.3rem; }
  .brand-nav:hover { background: hsl(var(--accent)); }
  .brand-text { color: hsl(var(--foreground)); }
  .brand-text .ai { color: hsl(var(--primary)); }
  /* Blocks that fill left to right while compiling. Steps, not a smooth bar. */
  .progress { position: absolute; left: 0; right: 0; bottom: -2px; height: 2px; overflow: hidden; background: transparent; }
  .progress i { display: block; height: 100%; width: 28%; background: repeating-linear-gradient(90deg, hsl(var(--primary)) 0 10px, transparent 10px 14px); animation: march 1.4s steps(14, end) infinite; }
  @keyframes march { from { transform: translateX(-100%); } to { transform: translateX(360%); } }
  @media (prefers-reduced-motion: reduce) { .progress i { animation: none; width: 100%; } }
</style>
