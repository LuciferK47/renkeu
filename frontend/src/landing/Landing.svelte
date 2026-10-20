<script>
  // The landing page. A fixed 3D scene (FoundryScene) sits behind transparent
  // sections; scrolling moves the camera down the tower, one floor per section.
  // Every word and control is ordinary DOM, so the page works with the scene
  // off, and each 3D reaction has a button that causes it.
  import { onMount } from 'svelte'
  import './landing.css'
  import FoundryScene from './FoundryScene.svelte'
  import PixelIcon from '../lib/PixelIcon.svelte'
  import Animatronic from '../lib/Animatronic.svelte'
  import Crab from '../lib/Crab.svelte'
  import Bot from '../lib/Bot.svelte'
  import { EngineStatus } from './engine.svelte.js'
  import { watchScroll, scrollToProgress } from './scroll.js'
  import { pickTier, detectEnvironment } from './scene/quality.js'
  import { actions, links, hero, loop, workspace, capabilities, engine as engineCopy, boundary, install, faq, closing, footer, floors, crew } from './copy.js'

  let { onopenapp = () => {} } = $props()

  const engine = new EngineStatus()
  const online = $derived(engine.state === 'online')

  let tier = $state('poster')
  let paused = $state(false)
  let position = $state(0)
  let sections = []
  let loopProgress = $state(0)
  let applied = $state(false)
  let discarded = $state(false)
  let asking = $state(false)
  let tab = $state('project')
  let copied = $state('')
  let openCrate = $state(null)

  const loopStep = $derived(Math.min(3, Math.max(0, Math.floor(loopProgress * 4))))
  const input = $derived({ step: loopStep, applied, asking, workspaceTab: tab, crate: engineCopy.crates.findIndex((c) => c.id === openCrate) })
  const floorLabel = $derived(floors[Math.min(floors.length - 1, Math.round(position))].label)
  const activeNote = $derived(workspace.tabs.find((t) => t.id === tab)?.note ?? '')

  onMount(() => {
    tier = pickTier(detectEnvironment())
    const stopEngine = engine.start()
    const stopScroll = watchScroll(
      () => sections,
      (p) => {
        position = p
        if (sections[1]) {
          const r = sections[1].getBoundingClientRect()
          const travel = Math.max(1, r.height - innerHeight)
          loopProgress = Math.min(1, Math.max(0, -r.top / travel))
        }
      }
    )
    return () => {
      stopEngine()
      stopScroll()
    }
  })

  async function copyCommand(id, text) {
    try {
      // Some browsers never answer when the page is not focused or has no
      // permission; report that as a failure instead of waiting forever.
      await Promise.race([
        navigator.clipboard.writeText(text),
        new Promise((_, reject) => setTimeout(() => reject(new Error('timeout')), 1500))
      ])
      copied = id
    } catch {
      copied = `${id}:failed`
    }
    setTimeout(() => {
      if (copied.startsWith(id)) copied = ''
    }, 1800)
  }

  function toStep(i) {
    if (sections[1]) scrollToProgress(sections[1], (i + 0.5) / 4)
  }

  function apply() {
    applied = true
    discarded = false
  }
  function discard() {
    applied = false
    discarded = true
  }

  const crateOf = (id) => engineCopy.crates.find((c) => c.id === id)
</script>

<div class="landing" data-tier={tier} class:paused>
  <FoundryScene {tier} {position} {input} {paused} floorCount={floors.length} />

  <a class="skip" href="#hero">Skip to content</a>

  <header class="hud" aria-label="Site">
    <a class="mark" href="#hero" aria-label="Renkeu, top of page">
      <span class="mark-glyph" aria-hidden="true"><PixelIcon name="crab" size={22} /></span>
      <span class="wordmark">Ren<span class="ai">keu</span></span>
    </a>
    <span class="floor" aria-live="off" title="Where you are in the tower">{floorLabel}</span>
    <nav aria-label="Sections">
      <a href="#loop">How it works</a>
      <a href="#workspace">Workspace</a>
      <a href="#engine">Rust inside</a>
      <a href="#boundary">Privacy</a>
      <a href="#install">Install</a>
      <a href="#faq">FAQ</a>
    </nav>
    <span class="chip" class:on={online} class:off={engine.state === 'offline'} role="status" title={online ? 'A Renkeu server answered' : 'No Renkeu server answered. Start one with: latex-ai serve'}>
      <i></i>
      {#if online}Engine online · {engine.latency} ms{:else if engine.state === 'offline'}Engine offline{:else}Checking…{/if}
    </span>
    {#if online}
      <button class="btn btn-primary" onclick={onopenapp}>{actions.open}</button>
    {:else}
      <a class="btn btn-primary" href="#install">{actions.get}</a>
    {/if}
    <button class="icon-btn" aria-pressed={paused} aria-label={paused ? 'Resume the animation' : 'Pause the animation'} onclick={() => (paused = !paused)} title={paused ? 'Resume animation' : 'Pause animation'}>
      <PixelIcon name={paused ? 'play' : 'pause'} size={16} />
    </button>
  </header>

  <main>
    <!-- Street -->
    <section id="hero" class="stage stage-hero" bind:this={sections[0]} aria-labelledby="hero-title">
      <div class="plate plate-hero">
        <p class="eyebrow">{hero.eyebrow}</p>
        <h1 id="hero-title" class="display">
          {#each hero.title as line, i (line)}<span class="line" class:accent={i === 2}>{line}</span>{/each}
        </h1>
        <p class="lede">{hero.body}</p>
        <div class="actions">
          {#if online}
            <button class="btn btn-primary btn-lg" onclick={onopenapp}>{actions.open} <PixelIcon name="arrow" size={16} /></button>
            <a class="btn btn-ghost btn-lg" href="#install">{actions.get}</a>
          {:else}
            <a class="btn btn-primary btn-lg" href="#install">{actions.get} <PixelIcon name="download" size={16} /></a>
          {/if}
          <a class="btn btn-ghost btn-lg" href={links.github} target="_blank" rel="noreferrer">{actions.source} <PixelIcon name="arrow" size={16} /></a>
        </div>
        <ul class="proof mono">
          {#each hero.proof as p (p)}<li>{p}</li>{/each}
        </ul>
        <p class="crew-note"><Animatronic id="bruno" scale={3} animate /><span><strong>{crew.bruno.name}</strong> {crew.bruno.line}</span></p>
      </div>
      <p class="scroll-cue mono" aria-hidden="true">scroll to descend <span>▼</span></p>
    </section>

    <!-- Floor 4: the repair loop (pinned) -->
    <section id="loop" class="stage stage-pin" bind:this={sections[1]} aria-labelledby="loop-title">
      <div class="pin">
        <div class="plate plate-left">
          <p class="eyebrow">{loop.eyebrow}</p>
          <h2 id="loop-title" class="display h2">{loop.title}</h2>
          <p class="lede">{loop.body}</p>
          <ol class="steps" aria-label="Steps of the repair loop">
            {#each loop.steps as step, i (step.id)}
              <li>
                <button class="step" class:active={i === loopStep} aria-current={i === loopStep ? 'step' : undefined} onclick={() => toStep(i)}>
                  <span class="n mono">0{i + 1}</span>
                  <span class="t">{step.label}</span>
                </button>
              </li>
            {/each}
          </ol>
          {#each loop.steps as step, i (step.id)}
            {#if i === loopStep}
              <div class="step-body" aria-live="polite">
                <h3>{step.title}</h3>
                <p>{step.body}</p>
                <p class="diag mono" class:good={i === 3 && applied}><span class="tag">{step.diagnostic}</span> <span class="where">{step.where}</span></p>
                {#if i === 3}
                  <div class="review" role="group" aria-label="Proposed changes (demo)">
                    <label class="diff-row"><input type="checkbox" checked disabled /> <code class="del">− The \oops sum</code></label>
                    <label class="diff-row"><input type="checkbox" checked disabled /> <code class="add">+ The \sum</code></label>
                    <div class="actions">
                      <button class="btn btn-primary" onclick={apply} disabled={applied}>{applied ? 'Applied (demo)' : 'Apply 1 change'}</button>
                      <button class="btn btn-ghost" onclick={discard}>Discard</button>
                    </div>
                    <p class="small">{applied ? 'The gate opens only because you pressed Apply. This is a demo: nothing was written anywhere.' : discarded ? 'Discarded. Nothing changed.' : 'The gate stays shut until you press Apply.'}</p>
                  </div>
                {/if}
              </div>
            {/if}
          {/each}
          <p class="crew-note"><Crab pose="carry" scale={3} /><Bot mood="working" scale={3} /><span><strong>{crew.repair.name}.</strong> {crew.repair.line}</span></p>
          <p class="small foot">{loop.footnote}</p>
        </div>
      </div>
    </section>

    <!-- Floor 3: the workspace -->
    <section id="workspace" class="stage stage-right" bind:this={sections[2]} aria-labelledby="ws-title">
      <div class="plate plate-right">
        <p class="eyebrow">{workspace.eyebrow}</p>
        <h2 id="ws-title" class="display h2">{workspace.title}</h2>
        <p class="lede">{workspace.body}</p>
        <div class="tabs" role="tablist" aria-label="Workspace demos">
          {#each workspace.tabs as t (t.id)}
            <button role="tab" id={`tab-${t.id}`} aria-selected={tab === t.id} aria-controls="ws-panel" class:on={tab === t.id} onclick={() => (tab = t.id)}>{t.label}</button>
          {/each}
        </div>
        <div id="ws-panel" role="tabpanel" aria-labelledby={`tab-${tab}`}><p class="tab-note">{activeNote}</p></div>
        <p class="crew-note"><Animatronic id="hazel" scale={3} animate /><span><strong>{crew.hazel.name}</strong> {crew.hazel.line}</span></p>
        <img class="shot" src={`${import.meta.env.BASE_URL}landing/editor.jpg`} alt="The Renkeu editor: file tree, source, PDF preview and assistant" width="960" height="540" loading="lazy" onerror={(e) => (e.currentTarget.hidden = true)} />
      </div>

      <div class="caps" aria-labelledby="caps-title">
        <div class="plate plate-wide">
          <p class="eyebrow">{capabilities.eyebrow}</p>
          <h2 id="caps-title" class="display h2">{capabilities.title}</h2>
          <ul class="cap-grid">
            {#each capabilities.items as item (item.id)}
              <li class="cap">
                <span class="cap-icon" aria-hidden="true"><PixelIcon name={{ files: 'folder', edit: 'tex', sync: 'target', ai: 'spark', visual: 'select', history: 'history' }[item.id]} size={24} /></span>
                <h3>{item.title}</h3>
                <p>{item.body}</p>
              </li>
            {/each}
          </ul>
          <div class="planned">
            <span class="tag mono">{capabilities.planned.label}</span>
            <ul>{#each capabilities.planned.items as p (p)}<li>{p}</li>{/each}</ul>
          </div>
        </div>
      </div>
    </section>

    <!-- Floor 2: the engine bench (pinned) -->
    <section id="engine" class="stage stage-pin stage-engine" bind:this={sections[3]} aria-labelledby="engine-title">
      <div class="pin pin-bottom">
        <div class="plate plate-bottom">
          <div class="cols">
            <div>
              <p class="eyebrow">{engineCopy.eyebrow}</p>
              <h2 id="engine-title" class="display h2">{engineCopy.title}</h2>
              <p class="lede">{engineCopy.body}</p>
              <p class="small note">{engineCopy.note}</p>
              <p class="crew-note"><Animatronic id="ember" scale={3} animate /><span><strong>{crew.ember.name}</strong> {crew.ember.line}</span></p>
            </div>
            <ul class="crates" aria-label="The six parts">
              {#each engineCopy.crates as c (c.id)}
                <li>
                  <button class="crate" aria-expanded={openCrate === c.id} onclick={() => (openCrate = openCrate === c.id ? null : c.id)} onmouseenter={() => (openCrate = c.id)} onfocus={() => (openCrate = c.id)}>
                    <span class="no mono">0{engineCopy.crates.indexOf(c) + 1}</span> <code class="mono">{c.name}</code>
                  </button>
                </li>
              {/each}
            </ul>
          </div>
          <p class="crate-role" aria-live="polite">{openCrate ? crateOf(openCrate).role : 'Select a part to see what it does.'}</p>
        </div>
      </div>
    </section>

    <!-- Floor 1: the vault -->
    <section id="boundary" class="stage stage-bottom" bind:this={sections[4]} aria-labelledby="b-title">
      <div class="plate plate-bottom">
        <div class="cols">
          <div>
            <p class="eyebrow">{boundary.eyebrow}</p>
            <h2 id="b-title" class="display h2">{boundary.title}</h2>
            <p class="lede">{boundary.body}</p>
            <button class="btn btn-ghost ask" aria-pressed={asking} onclick={() => (asking = !asking)}>
              <PixelIcon name="spark" size={16} /> {asking ? 'The AI is being asked…' : 'Ask the AI'}
            </button>
            <p class="small"><strong>{boundary.local}</strong> {boundary.small}</p>
            <p class="crew-note"><Animatronic id="aurum" scale={3} animate /><span><strong>{crew.aurum.name}</strong> {crew.aurum.line}</span></p>
          </div>
          <div class="sent" class:lit={asking}>
            <p class="tag mono">{asking ? 'Crossing the boundary now' : 'Would cross the boundary if you asked'}</p>
            <ul>{#each boundary.sent as s (s)}<li>{s}</li>{/each}</ul>
            <p class="small">{boundary.sentVisual}</p>
          </div>
        </div>
      </div>
    </section>

    <!-- Dock -->
    <section id="install" class="stage stage-bottom" bind:this={sections[5]} aria-labelledby="install-title">
      <div class="plate plate-bottom">
        <p class="eyebrow">{install.eyebrow}</p>
        <h2 id="install-title" class="display h2">{install.title}</h2>
        <p class="crew-note"><Animatronic id="pip" scale={3} animate /><span><strong>{crew.pip.name}</strong> {crew.pip.line}</span></p>
        <ul class="bays">
          {#each install.items as item, i (item.id)}
            <li class="bay">
              <span class="bay-no mono">0{i + 1}</span>
              <h3>{item.name}</h3>
              <p>{item.note}</p>
              {#if item.command}
                <div class="cmd">
                  <code class="mono">{item.command}</code>
                  <button class="icon-btn" aria-label={`Copy the ${item.name} command`} onclick={() => copyCommand(item.id, item.command)}><PixelIcon name="copy" size={16} /></button>
                </div>
                <p class="small copied" role="status">{copied === item.id ? 'Copied.' : copied === `${item.id}:failed` ? 'Could not copy: select the command and copy it.' : ''}</p>
              {:else}
                <a class="btn btn-ghost" href={links.releases} target="_blank" rel="noreferrer">{item.cta} <PixelIcon name="arrow" size={16} /></a>
              {/if}
            </li>
          {/each}
        </ul>
      </div>
    </section>

    <!-- Street at dawn -->
    <section id="faq" class="stage stage-faq" aria-labelledby="faq-title">
      <div class="plate plate-wide">
        <p class="eyebrow">{faq.eyebrow}</p>
        <h2 id="faq-title" class="display h2">{faq.title}</h2>
        <div class="faq">
          {#each faq.items as [q, a] (q)}
            <details>
              <summary>{q}</summary>
              <p>{a}</p>
            </details>
          {/each}
        </div>
      </div>
    </section>

    <section id="closing" class="stage stage-closing" bind:this={sections[6]} aria-labelledby="closing-title">
      <div class="plate plate-hero">
        <h2 id="closing-title" class="display">
          {#each closing.title as line, i (line)}<span class="line" class:accent={i === 2}>{line}</span>{/each}
        </h2>
        <div class="actions">
          {#if online}
            <button class="btn btn-primary btn-lg" onclick={onopenapp}>{actions.open} <PixelIcon name="arrow" size={16} /></button>
          {:else}
            <a class="btn btn-primary btn-lg" href="#install">{actions.get}</a>
          {/if}
          <a class="btn btn-ghost btn-lg" href={links.github} target="_blank" rel="noreferrer">{actions.source}</a>
        </div>
      </div>
    </section>
  </main>

  <footer class="foot-bar mono">
    <span>{footer}</span>
    <a href={links.github} target="_blank" rel="noreferrer">GitHub</a>
    <a href={links.releases} target="_blank" rel="noreferrer">Releases</a>
    <a href="#hero">Back to the street ↑</a>
  </footer>
</div>
