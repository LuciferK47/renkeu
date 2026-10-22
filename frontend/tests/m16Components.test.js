import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte'

vi.mock('../src/lib/api/client.js', () => ({
  api: {
    previewHunks: vi.fn(),
    getConfig: vi.fn(),
    saveConfig: vi.fn(),
    setApiKey: vi.fn(),
    clearApiKey: vi.fn(),
    getHistory: vi.fn(),
    searchWorkspace: vi.fn(),
    getLocalModels: vi.fn(() => Promise.resolve({ running: false, models: [] })),
  },
  APIError: class extends Error {},
}))

import { api } from '../src/lib/api/client.js'
import RunTimeline from '../src/components/RunTimeline.svelte'
import AssistantPane from '../src/components/AssistantPane.svelte'
import ProposalReview from '../src/components/ProposalReview.svelte'
import SettingsDialog from '../src/components/SettingsDialog.svelte'
import HistoryPanel from '../src/components/HistoryPanel.svelte'
import AskDialog from '../src/components/AskDialog.svelte'
import QuickOpen from '../src/components/QuickOpen.svelte'
import { PaperState } from '../src/lib/state/paper.svelte.js'
import { UiState } from '../src/lib/state/ui.svelte.js'
import { CommandRegistry } from '../src/lib/commands.js'

beforeEach(() => {
  vi.clearAllMocks()
  localStorage.clear()
})

describe('RunTimeline', () => {
  it('lists each step with its state and the way the run ended', () => {
    const run = {
      steps: [
        { id: 1, kind: 'thinking', text: 'Attempt 1', detail: 'reading the error', state: 'ok' },
        { id: 2, kind: 'applying', text: 'It did not compile', detail: 'Missing $', state: 'failed' },
        { id: 3, kind: 'thinking', text: 'Attempt 2', detail: '', state: 'active' },
      ],
      outcome: 'cancelled',
      note: '',
    }
    render(RunTimeline, { props: { run } })
    const items = screen.getAllByRole('listitem').map((li) => li.textContent.replace(/\s+/g, ' ').trim())
    expect(items[0]).toContain('✓ Attempt 1')
    expect(items[1]).toContain('✕ It did not compile')
    expect(items[1]).toContain('Missing $')
    expect(items[2]).toContain('… Attempt 2')
    expect(items[3]).toBe('Stopped.')
    expect(screen.getByRole('status').textContent).toBe('in progress')
  })

  it('renders what the model wrote as text, never as markup', () => {
    const run = { steps: [{ id: 1, kind: 'error', text: '<img src=x onerror=alert(1)>', detail: '<b>bold</b>', state: 'failed' }], outcome: null, note: '' }
    const { container } = render(RunTimeline, { props: { run } })
    expect(container.querySelector('img')).toBeNull()
    expect(container.querySelector('b')).toBeNull()
    expect(container.textContent).toContain('<img src=x onerror=alert(1)>')
  })

  it('puts the reason after a failed ending', () => {
    render(RunTimeline, { props: { run: { steps: [], outcome: 'failed', note: 'Max iterations reached' } } })
    expect(screen.getByText(/Max iterations reached/)).toBeTruthy()
  })
})

describe('AssistantPane', () => {
  const fakeAi = (over = {}) => ({
    messages: [],
    proposal: null,
    status: 'idle',
    running: false,
    send: vi.fn(),
    cancel: vi.fn(),
    newConversation: vi.fn(),
    apply: vi.fn(),
    discard: vi.fn(),
    ...over,
  })

  it('explains how to turn the assistant on and blocks sending when there is no key', async () => {
    const onsettings = vi.fn()
    render(AssistantPane, { props: { ai: fakeAi(), aiEnabled: false, hasFile: true, onsettings } })
    expect(screen.getByRole('note').textContent).toMatch(/API key/)
    expect(screen.getByRole('textbox').disabled).toBe(true)
    await fireEvent.click(screen.getByRole('button', { name: 'Open settings' }))
    expect(onsettings).toHaveBeenCalled()
  })

  it('sends on Enter, keeps Shift+Enter for a new line, and clears the box', async () => {
    const onsend = vi.fn()
    render(AssistantPane, { props: { ai: fakeAi(), aiEnabled: true, hasFile: true, onsend } })
    const box = screen.getByRole('textbox')
    await fireEvent.input(box, { target: { value: 'add a title' } })
    await fireEvent.keyDown(box, { key: 'Enter', shiftKey: true })
    expect(onsend).not.toHaveBeenCalled()
    await fireEvent.keyDown(box, { key: 'Enter' })
    expect(onsend).toHaveBeenCalledWith('add a title')
    expect(box.value).toBe('')
  })

  it('shows Stop while a run is going and calls cancel', async () => {
    const ai = fakeAi({ running: true, status: 'running' })
    render(AssistantPane, { props: { ai, aiEnabled: true, hasFile: true } })
    expect(screen.queryByRole('button', { name: 'Send' })).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: 'Stop' }))
    expect(ai.cancel).toHaveBeenCalled()
  })

  it('needs an open file, and suggestions send straight away', async () => {
    const onsend = vi.fn()
    const { unmount } = render(AssistantPane, { props: { ai: fakeAi(), aiEnabled: true, hasFile: false, onsend } })
    expect(screen.getByRole('textbox').disabled).toBe(true)
    unmount()
    render(AssistantPane, { props: { ai: fakeAi(), aiEnabled: true, hasFile: true, onsend } })
    await fireEvent.click(screen.getByRole('button', { name: /table of contents/ }))
    expect(onsend).toHaveBeenCalledWith('Add a table of contents')
  })

  it('shows each request with its timeline, and a new chat resets', async () => {
    const ai = fakeAi({
      messages: [
        { id: 1, role: 'user', text: 'fix it' },
        { id: 2, role: 'assistant', run: { steps: [{ id: 1, kind: 'thinking', text: 'Attempt 1', detail: '', state: 'ok' }], outcome: 'success', note: '' } },
      ],
    })
    render(AssistantPane, { props: { ai, aiEnabled: true, hasFile: true } })
    expect(screen.getByText('fix it')).toBeTruthy()
    expect(screen.getByText('Attempt 1')).toBeTruthy()
    await fireEvent.click(screen.getByRole('button', { name: 'New chat' }))
    expect(ai.newConversation).toHaveBeenCalled()
  })
})

describe('ProposalReview', () => {
  const edits = [
    { file_path: 'main.tex', hunk_id: 'h1', op: 'replace_range' },
    { file_path: 'ch/one.tex', hunk_id: 'h2', op: 'insert_after' },
    { file_path: 'ch/one.tex', hunk_id: 'h3', op: 'replace_range' },
  ]
  const hunks = [
    { hunk_id: 'h1', diff: '@@\n-old\n+new', applies: true },
    { hunk_id: 'h2', diff: '@@\n+added', applies: true },
    { hunk_id: 'h3', diff: '', applies: false, error: 'the text changed' },
  ]
  const proposal = { file: 'main.tex', edits, verified: true, etag: 'e1' }

  beforeEach(() => api.previewHunks.mockResolvedValue({ etag: 'preview-etag', hunks }))

  it('groups hunks by file and previews them against the active file', async () => {
    render(ProposalReview, { props: { proposal } })
    await screen.findByText('ch/one.tex')
    expect(api.previewHunks).toHaveBeenCalledWith('main.tex', edits)
    expect(screen.getByText('main.tex')).toBeTruthy()
    expect(screen.getByText('These edits compile.')).toBeTruthy()
    expect(screen.getByText('the text changed')).toBeTruthy()
    expect(screen.getByText('1 no longer apply')).toBeTruthy()
  })

  it('applies exactly the selected hunks that still apply', async () => {
    const onapply = vi.fn()
    render(ProposalReview, { props: { proposal, onapply } })
    await screen.findByText('ch/one.tex')
    expect(screen.getByRole('button', { name: 'Apply 2 changes' })).toBeTruthy()

    await fireEvent.click(screen.getByLabelText('Change h2 in ch/one.tex'))
    await fireEvent.click(screen.getByRole('button', { name: 'Apply 1 change' }))
    expect(onapply).toHaveBeenCalledWith({ edits, selectedHunkIds: ['h1'], expectedEtag: 'preview-etag' })
  })

  it('a file checkbox takes or leaves that file alone', async () => {
    render(ProposalReview, { props: { proposal } })
    await screen.findByText('ch/one.tex')
    await fireEvent.click(screen.getByLabelText('Select every change in main.tex'))
    expect(screen.getByRole('button', { name: 'Apply 1 change' })).toBeTruthy()
    expect(screen.getByText('1 of 2 selected')).toBeTruthy()
  })

  it('discard and a failed preview', async () => {
    const ondiscard = vi.fn()
    api.previewHunks.mockRejectedValueOnce(new Error('preview failed'))
    render(ProposalReview, { props: { proposal, ondiscard } })
    expect((await screen.findByRole('alert')).textContent).toContain('preview failed')
    expect(screen.getByRole('button', { name: 'Apply 0 changes' }).disabled).toBe(true)
    await fireEvent.click(screen.getByRole('button', { name: 'Discard' }))
    expect(ondiscard).toHaveBeenCalled()
  })

  it('stops selecting and applying while an apply is in progress', async () => {
    render(ProposalReview, { props: { proposal, busy: true } })
    await screen.findByText('ch/one.tex')
    expect(screen.getByRole('button', { name: 'Applying…' }).disabled).toBe(true)
    expect(screen.getByLabelText('Change h1 in main.tex').disabled).toBe(true)
  })
})

describe('SettingsDialog', () => {
  const server = (over = {}) => ({ ai_enabled: true, model: 'm1', vision_model: null, timeout_secs: 120, key_source: 'file', env_overridden: [], ...over })
  const mount = (extra = {}) => {
    const ui = new UiState()
    const documents = { autosave: true }
    const commands = new CommandRegistry()
    commands.register({ id: 'save', title: 'Save file', keys: 'Mod+S', run() {} })
    commands.register({ id: 'refresh', title: 'Refresh', run() {} })
    const onchange = vi.fn()
    const paper = new PaperState()
    render(SettingsDialog, { props: { ui, documents, commands, paper, onchange, ...extra } })
    return { ui, documents, onchange, paper }
  }
  const openTab = (name) => fireEvent.click(screen.getByRole('tab', { name }))

  it('changes the theme and editor preferences immediately, within bounds', async () => {
    api.getConfig.mockResolvedValue(server())
    const { ui, documents } = mount()
    await fireEvent.change(screen.getByLabelText('Theme'), { target: { value: 'dark' } })
    expect(ui.theme).toBe('dark')

    await openTab('Editor')
    await fireEvent.change(screen.getByLabelText('Font size'), { target: { value: '99' } })
    expect(ui.editor.fontSize).toBe(28)
    await fireEvent.change(screen.getByLabelText('Tab width'), { target: { value: '8' } })
    expect(ui.editor.tabSize).toBe(8)
    await fireEvent.click(screen.getByLabelText('Wrap long lines'))
    expect(ui.editor.wordWrap).toBe(false)
    await fireEvent.click(screen.getByLabelText('Save automatically after a pause'))
    expect(documents.autosave).toBe(false)
  })

  it('locks a field pinned by an environment variable and never sends it', async () => {
    api.getConfig.mockResolvedValue(server({ env_overridden: ['model'] }))
    api.saveConfig.mockResolvedValue(server({ env_overridden: ['model'], timeout_secs: 90 }))
    const { onchange } = mount()
    await openTab('Assistant')
    const model = await screen.findByLabelText('Model')
    expect(model.disabled).toBe(true)
    expect(screen.getByText(/Set by LATEX_AI_MODEL/)).toBeTruthy()

    await fireEvent.input(screen.getByLabelText('Request timeout (seconds)'), { target: { value: '90' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(api.saveConfig).toHaveBeenCalledWith({ timeout_secs: 90 }))
    await waitFor(() => expect(onchange).toHaveBeenCalled())
  })

  it('does not call the server when nothing changed', async () => {
    api.getConfig.mockResolvedValue(server())
    mount()
    await openTab('Assistant')
    await screen.findByLabelText('Model')
    await fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    expect(api.saveConfig).not.toHaveBeenCalled()
    expect(screen.getByRole('status').textContent).toBe('Nothing to save.')
  })

  it('stores a key write-only: sent once, cleared from the box, never displayed', async () => {
    api.getConfig.mockResolvedValue(server({ key_source: 'none', ai_enabled: false }))
    api.setApiKey.mockResolvedValue(server({ key_source: 'file' }))
    const { onchange } = mount()
    await openTab('Assistant')
    const key = await screen.findByLabelText('Groq API key')
    expect(key.type).toBe('password')
    expect(screen.getByRole('button', { name: 'Save key' }).disabled).toBe(true)

    await fireEvent.input(key, { target: { value: ' gsk_secret ' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Save key' }))
    await waitFor(() => expect(api.setApiKey).toHaveBeenCalledWith('gsk_secret'))
    await waitFor(() => expect(screen.getByLabelText('Replace key').value).toBe(''))
    expect(document.body.textContent).not.toContain('gsk_secret')
    expect(onchange).toHaveBeenCalled()
  })

  it('shows a failed save and an environment-supplied key as read-only', async () => {
    api.getConfig.mockResolvedValue(server({ key_source: 'env' }))
    mount()
    await openTab('Assistant')
    expect(await screen.findByText(/GROQ_API_KEY environment variable/)).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Save key' })).toBeNull()
  })

  it('reports an unreachable server instead of showing empty fields', async () => {
    api.getConfig.mockRejectedValue(new Error('connection refused'))
    mount()
    await openTab('Assistant')
    expect((await screen.findByRole('alert')).textContent).toContain('connection refused')
  })

  it('lists the shortcuts the app really has', async () => {
    api.getConfig.mockResolvedValue(server())
    mount()
    await openTab('Shortcuts')
    expect(screen.getByText('Save file')).toBeTruthy()
    expect(screen.queryByText('Refresh')).toBeNull() // no shortcut, so not listed
  })
})

describe('SettingsDialog: page tone', () => {
  it('drives the dark-page tone through the sliders and can reset it', async () => {
    api.getConfig.mockResolvedValue({ ai_enabled: true, model: 'm', vision_model: null, timeout_secs: 120, key_source: 'file', env_overridden: [] })
    const ui = new UiState()
    const paper = new PaperState()
    render(SettingsDialog, { props: { ui, documents: { autosave: true }, commands: new CommandRegistry(), paper } })

    await fireEvent.input(screen.getByLabelText('Brightness'), { target: { value: '0.7' } })
    await fireEvent.input(screen.getByLabelText('Warmth'), { target: { value: '30' } })
    expect([paper.paper, paper.warmth]).toEqual([0.7, 30])
    expect(screen.getByText('70%')).toBeTruthy()

    await fireEvent.click(screen.getByRole('button', { name: 'Reset page tone' }))
    expect(paper.paper).toBe(0.88)
  })
})

describe('QuickOpen', () => {
  const hits = (...v) => ({ hits: v.map((value) => ({ value, score: 1 })) })

  it('lists recent files first when nothing is typed, and opens one with Enter', async () => {
    api.searchWorkspace.mockResolvedValue(hits('a.tex', 'b.tex', 'ch/c.tex'))
    const onopen = vi.fn()
    render(QuickOpen, { props: { recent: ['ch/c.tex'], onopen } })
    await screen.findByText('b.tex')
    expect(screen.getAllByRole('option').map((o) => o.textContent.replace(/\s+/g, ' ').trim())).toEqual(['ch/c.tex recent', 'a.tex', 'b.tex'])

    await fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Enter' })
    expect(onopen).toHaveBeenCalledWith('ch/c.tex')
  })

  it('searches as you type and moves with the arrow keys', async () => {
    api.searchWorkspace.mockResolvedValueOnce(hits('a.tex', 'b.tex')).mockResolvedValue(hits('chapters/intro.tex', 'chapters/intro2.tex'))
    const onopen = vi.fn()
    render(QuickOpen, { props: { onopen } })
    await screen.findByText('a.tex')
    await fireEvent.input(screen.getByLabelText('Search files'), { target: { value: 'intro' } })
    await screen.findByText('chapters/intro.tex')
    expect(api.searchWorkspace).toHaveBeenLastCalledWith('intro')

    const dialog = screen.getByRole('dialog')
    await fireEvent.keyDown(dialog, { key: 'ArrowDown' })
    await fireEvent.keyDown(dialog, { key: 'ArrowDown' }) // already last: stays
    await fireEvent.keyDown(dialog, { key: 'Enter' })
    expect(onopen).toHaveBeenCalledWith('chapters/intro2.tex')
  })

  it('Enter pressed before the answer to what was typed arrives opens that answer, not the stale list', async () => {
    let answer
    api.searchWorkspace.mockResolvedValueOnce(hits('recent.tex', 'refs.bib')).mockImplementationOnce(() => new Promise((r) => (answer = r)))
    const onopen = vi.fn()
    render(QuickOpen, { props: { recent: ['recent.tex'], onopen } })
    await screen.findByText('refs.bib')

    await fireEvent.input(screen.getByLabelText('Search files'), { target: { value: 'refs' } })
    await waitFor(() => expect(api.searchWorkspace).toHaveBeenCalledTimes(2))
    await fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Enter' })
    expect(onopen).not.toHaveBeenCalled() // the visible top entry is still recent.tex

    answer(hits('refs.bib'))
    await waitFor(() => expect(onopen).toHaveBeenCalledWith('refs.bib'))
  })

  it('says so when nothing matches, shows a failure, and Escape closes', async () => {
    const onclose = vi.fn()
    api.searchWorkspace.mockResolvedValueOnce(hits()).mockRejectedValueOnce(new Error('server down'))
    render(QuickOpen, { props: { onclose } })
    expect(await screen.findByText('No files yet.')).toBeTruthy()
    await fireEvent.input(screen.getByLabelText('Search files'), { target: { value: 'zzz' } })
    expect((await screen.findByRole('alert')).textContent).toContain('server down')
    await fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' })
    expect(onclose).toHaveBeenCalled()
  })
})

describe('HistoryPanel', () => {
  const entries = [
    { id: 3, file_path: 'a.tex', edit_type: 'apply-selected', diff: '@@\n-x\n+y', created_at: '2026-10-18 12:00:00', has_before: true },
    { id: 2, file_path: 'a.tex', edit_type: 'delete', diff: '(binary file a.tex removed)', created_at: '2026-10-18 11:00:00', has_before: true },
    { id: 1, file_path: 'a.tex', edit_type: 'manual-save', diff: '@@', created_at: '2026-10-18 10:00:00', has_before: false },
  ]
  beforeEach(() => api.getHistory.mockResolvedValue(entries))

  it("lists this file's entries with plain-language kinds, newest first", async () => {
    render(HistoryPanel, { props: { filePath: 'a.tex' } })
    await screen.findByText('AI change')
    expect(api.getHistory).toHaveBeenCalledWith({ path: 'a.tex', agent_only: undefined, limit: 200 })
    expect(screen.getByText('Deleted')).toBeTruthy()
    expect(screen.getAllByRole('button', { name: /Restore|Bring back|Undo/ }).map((b) => b.textContent.trim())).toEqual([
      'Undo this change', // the AI change kept what it replaced
      'Restore this version',
      'Bring back', // a delete can only be undone
      'Restore this version', // an old entry kept nothing earlier: no undo offered
    ])
  })

  it('filters by scope and by AI changes', async () => {
    render(HistoryPanel, { props: { filePath: 'a.tex' } })
    await screen.findByText('AI change')
    await fireEvent.click(screen.getByLabelText('All files'))
    await waitFor(() => expect(api.getHistory).toHaveBeenLastCalledWith({ path: undefined, agent_only: undefined, limit: 200 }))
    await fireEvent.click(screen.getByLabelText('AI changes only'))
    await waitFor(() => expect(api.getHistory).toHaveBeenLastCalledWith({ path: undefined, agent_only: 'true', limit: 200 }))
  })

  it('without an open file it starts on all files', async () => {
    render(HistoryPanel, { props: { filePath: '' } })
    await screen.findByText('AI change')
    expect(api.getHistory).toHaveBeenCalledWith({ path: undefined, agent_only: undefined, limit: 200 })
  })

  it('expands an entry to show its diff', async () => {
    render(HistoryPanel, { props: { filePath: 'a.tex' } })
    await fireEvent.click(await screen.findByText('AI change'))
    expect(document.querySelector('.diff').textContent).toContain('+y')
  })

  it('restores through the callback and refreshes the list when it worked', async () => {
    const onrestore = vi.fn(async () => true)
    render(HistoryPanel, { props: { filePath: 'a.tex', onrestore } })
    await screen.findByText('AI change')
    await fireEvent.click(screen.getAllByRole('button', { name: 'Restore this version' })[0])
    await waitFor(() => expect(onrestore).toHaveBeenCalledWith(entries[0], 'after'))
    await waitFor(() => expect(api.getHistory).toHaveBeenCalledTimes(2))
  })

  it('undo asks for the version before the change', async () => {
    const onrestore = vi.fn(async () => false)
    render(HistoryPanel, { props: { filePath: 'a.tex', onrestore } })
    await screen.findByText('AI change')
    await fireEvent.click(screen.getByRole('button', { name: 'Undo this change' }))
    await waitFor(() => expect(onrestore).toHaveBeenCalledWith(entries[0], 'before'))
    // A failed restore does not reload the list.
    expect(api.getHistory).toHaveBeenCalledTimes(1)
  })

  it('shows a load failure and an empty history plainly', async () => {
    api.getHistory.mockRejectedValueOnce(new Error('server down'))
    const { unmount } = render(HistoryPanel, { props: { filePath: 'a.tex' } })
    expect((await screen.findByRole('alert')).textContent).toContain('server down')
    unmount()
    api.getHistory.mockResolvedValueOnce([])
    render(HistoryPanel, { props: { filePath: 'a.tex' } })
    expect(await screen.findByText('Nothing here yet.')).toBeTruthy()
  })
})

describe('AskDialog', () => {
  it('Enter is a quick plan; the other button verifies; both need text', async () => {
    const onplan = vi.fn()
    const onrun = vi.fn()
    render(AskDialog, { props: { onplan, onrun } })
    expect(screen.getByRole('button', { name: 'Quick plan' }).disabled).toBe(true)
    const input = screen.getByLabelText('What should change')
    await fireEvent.input(input, { target: { value: '  tighten the margins ' } })
    await fireEvent.submit(input.closest('form'))
    expect(onplan).toHaveBeenCalledWith('tighten the margins')
    await fireEvent.click(screen.getByRole('button', { name: 'Plan and verify' }))
    expect(onrun).toHaveBeenCalledWith('tighten the margins')
  })

  it('closes on Escape', async () => {
    const onclose = vi.fn()
    render(AskDialog, { props: { onclose } })
    await fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' })
    expect(onclose).toHaveBeenCalled()
  })
})
