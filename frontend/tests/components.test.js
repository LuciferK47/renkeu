import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte'

vi.mock('../src/lib/api/client.js', () => ({
  api: {
    getBuildLog: vi.fn(),
    pdfPageUrl: (file, n, dpi, tone, build) => `/page?${build ? `build=${build}` : `file=${file}`}&n=${n}&dpi=${dpi}`,
    exportPdfUrl: (id) => `/export/${id}`,
    rawUrl: (p) => `/raw?path=${encodeURIComponent(p)}`,
  },
  APIError: class extends Error {},
}))

import { api } from '../src/lib/api/client.js'
import ProjectTree from '../src/components/ProjectTree.svelte'
import ProblemsPanel from '../src/components/ProblemsPanel.svelte'
import ConflictDialog from '../src/components/ConflictDialog.svelte'
import CommandMenu from '../src/components/CommandMenu.svelte'
import PdfPane from '../src/components/PdfPane.svelte'
import Toolbar from '../src/components/Toolbar.svelte'
import { CommandRegistry } from '../src/lib/commands.js'
import { ProjectState } from '../src/lib/state/project.svelte.js'
import { CompileState } from '../src/lib/state/compile.svelte.js'
import { UiState } from '../src/lib/state/ui.svelte.js'

const tree = [
  { name: 'chapters', path: 'chapters', kind: 'dir', editable: false, children: [
    { name: 'one.tex', path: 'chapters/one.tex', kind: 'file', editable: true },
    { name: 'fig.png', path: 'chapters/fig.png', kind: 'file', editable: false },
  ] },
  { name: 'main.tex', path: 'main.tex', kind: 'file', editable: true },
]

function projectWith(t = tree) {
  const p = new ProjectState({ api: { setMain: async () => ({}) } })
  p.tree = t
  return p
}

describe('ProjectTree', () => {
  const documents = { activePath: 'main.tex', get: (p) => (p === 'main.tex' ? { dirty: true } : null) }

  it('shows top-level entries, opens a directory on click, and opens a file', async () => {
    const project = projectWith()
    const onopen = vi.fn()
    render(ProjectTree, { props: { project, documents, onopen } })

    expect(screen.getAllByRole('treeitem').map((e) => e.textContent.trim())).toEqual([
      expect.stringContaining('chapters'),
      expect.stringContaining('main.tex'),
    ])

    await fireEvent.click(screen.getByRole('treeitem', { name: /chapters/ }))
    expect(screen.getByRole('treeitem', { name: /one\.tex/ })).toBeTruthy()

    await fireEvent.click(screen.getByRole('treeitem', { name: /one\.tex/ }))
    expect(onopen).toHaveBeenCalledWith('chapters/one.tex')
  })

  it('previews an image instead of opening it as text, and marks unsaved and active files', async () => {
    const project = projectWith()
    project.expanded = new Set(['chapters'])
    const onopen = vi.fn()
    render(ProjectTree, { props: { project, documents, onopen } })

    const png = screen.getByRole('treeitem', { name: /fig\.png/ })
    expect(png.disabled).toBe(false)
    await fireEvent.click(png)
    expect(onopen).not.toHaveBeenCalled()
    expect(screen.getByRole('dialog', { name: /Preview of chapters\/fig\.png/ })).toBeTruthy()
    expect(screen.getByRole('img').getAttribute('src')).toContain('/raw?path=chapters%2Ffig.png')

    const main = screen.getByRole('treeitem', { name: /main\.tex/ })
    expect(main.getAttribute('aria-selected')).toBe('true')
    expect(main.textContent).toContain('●') // unsaved
  })

  it('offers file actions only when a file manager is attached', async () => {
    render(ProjectTree, { props: { project: projectWith(), documents } })
    expect(screen.queryByRole('button', { name: 'New file' })).toBeNull()
    await fireEvent.contextMenu(screen.getByRole('treeitem', { name: /main\.tex/ }))
    expect(screen.queryByRole('menu')).toBeNull()
  })

  it('right-click offers rename, delete and main-file choice for a .tex file', async () => {
    const project = projectWith()
    const ops = { create: vi.fn(), rename: vi.fn(), remove: vi.fn(), upload: vi.fn() }
    render(ProjectTree, { props: { project, documents, ops } })

    await fireEvent.contextMenu(screen.getByRole('treeitem', { name: /main\.tex/ }))
    const items = screen.getAllByRole('menuitem').map((i) => i.textContent.trim())
    expect(items).toEqual(['New file…', 'Upload files…', 'Rename…', 'Set as main file', 'Delete…'])

    await fireEvent.click(screen.getByRole('menuitem', { name: 'Set as main file' }))
    expect(project.mainOverride).toBe('main.tex')
  })

  it('renames through a prompt that rejects a path leaving the project', async () => {
    const ops = { create: vi.fn(), rename: vi.fn(async () => true), remove: vi.fn(), upload: vi.fn() }
    render(ProjectTree, { props: { project: projectWith(), documents, ops } })

    await fireEvent.contextMenu(screen.getByRole('treeitem', { name: /main\.tex/ }))
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Rename…' }))
    const input = screen.getByRole('textbox', { name: 'Name' })
    expect(input.value).toBe('main.tex')

    await fireEvent.input(input, { target: { value: '../escape.tex' } })
    expect(screen.getByRole('button', { name: 'Rename' }).disabled).toBe(true)

    await fireEvent.input(input, { target: { value: 'paper.tex' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Rename' }))
    expect(ops.rename).toHaveBeenCalledWith('main.tex', 'paper.tex')
  })

  it('new file in a folder starts from that folder', async () => {
    const ops = { create: vi.fn(async () => 'chapters/untitled.tex'), rename: vi.fn(), remove: vi.fn(), upload: vi.fn() }
    render(ProjectTree, { props: { project: projectWith(), documents, ops } })

    await fireEvent.contextMenu(screen.getByRole('treeitem', { name: /chapters/ }))
    await fireEvent.click(screen.getByRole('menuitem', { name: 'New file…' }))
    expect(screen.getByRole('textbox', { name: 'Name' }).value).toBe('chapters/untitled.tex')
    await fireEvent.click(screen.getByRole('button', { name: 'Create' }))
    expect(ops.create).toHaveBeenCalledWith('chapters/untitled.tex')
  })

  it('deleting asks first, and does nothing if declined', async () => {
    const ops = { create: vi.fn(), rename: vi.fn(), remove: vi.fn(async () => 1), upload: vi.fn() }
    render(ProjectTree, { props: { project: projectWith(), documents, ops } })
    const confirm = vi.spyOn(window, 'confirm')

    await fireEvent.contextMenu(screen.getByRole('treeitem', { name: /main\.tex/ }))
    confirm.mockReturnValueOnce(false)
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Delete…' }))
    expect(ops.remove).not.toHaveBeenCalled()

    await fireEvent.contextMenu(screen.getByRole('treeitem', { name: /main\.tex/ }))
    confirm.mockReturnValueOnce(true)
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Delete…' }))
    expect(ops.remove).toHaveBeenCalledWith(expect.objectContaining({ path: 'main.tex' }))
    confirm.mockRestore()
  })

  it('dropping files uploads them into the folder they were dropped on', async () => {
    const ops = { create: vi.fn(), rename: vi.fn(), remove: vi.fn(), upload: vi.fn() }
    render(ProjectTree, { props: { project: projectWith(), documents, ops } })
    const file = new File(['x'], 'plot.png', { type: 'image/png' })

    await fireEvent.drop(screen.getByRole('treeitem', { name: /chapters/ }), {
      dataTransfer: { files: [file], types: ['Files'] },
    })
    expect(ops.upload).toHaveBeenCalledWith([file], 'chapters')

    await fireEvent.drop(screen.getByRole('treeitem', { name: /main\.tex/ }), {
      dataTransfer: { files: [file], types: ['Files'] },
    })
    expect(ops.upload).toHaveBeenLastCalledWith([file], '')
  })

  it('explains an empty project and a failed listing', () => {
    const empty = projectWith([])
    const { unmount } = render(ProjectTree, { props: { project: empty, documents } })
    expect(screen.getByText(/no files yet/i)).toBeTruthy()
    unmount()

    const broken = projectWith([])
    broken.error = 'server down'
    render(ProjectTree, { props: { project: broken, documents } })
    expect(screen.getByRole('alert').textContent).toContain('server down')
  })
})

describe('ProblemsPanel', () => {
  const compileWith = (over = {}) => {
    const c = new CompileState({ api: {} })
    Object.assign(c, over)
    return c
  }

  beforeEach(() => vi.clearAllMocks())

  it('lists errors then warnings, each a link to its line', async () => {
    const compile = compileWith({
      status: 'error',
      errors: [{ message: 'Undefined control sequence.', file: 'ch/a.tex', line: 2, context: '\\oops' }],
      warnings: [{ message: 'Overfull hbox', file: 'main.tex', line: 9 }],
    })
    const onreveal = vi.fn()
    render(ProblemsPanel, { props: { compile, onreveal } })

    const items = screen.getAllByRole('button', { name: /error:|warning:/ })
    expect(items).toHaveLength(2)
    expect(items[0].textContent).toContain('ch/a.tex:2')
    await fireEvent.click(items[0])
    expect(onreveal).toHaveBeenCalledWith({ file: 'ch/a.tex', line: 2 })
  })

  it('a problem with no file is shown but cannot be clicked', () => {
    render(ProblemsPanel, { props: { compile: compileWith({ status: 'error', errors: [{ message: 'engine failed' }] }) } })
    expect(screen.getByRole('button', { name: /engine failed/ }).disabled).toBe(true)
  })

  it('says what state it is in when there are no problems', () => {
    const { unmount } = render(ProblemsPanel, { props: { compile: compileWith({ status: 'idle' }) } })
    expect(screen.getByText('Nothing compiled yet.')).toBeTruthy()
    unmount()
    render(ProblemsPanel, { props: { compile: compileWith({ status: 'success' }) } })
    expect(screen.getByText('Compiled with no problems.')).toBeTruthy()
  })

  it('asks the user to choose when the project has several roots', () => {
    render(ProblemsPanel, { props: { compile: compileWith({ status: 'error', candidates: ['a.tex', 'b.tex'] }) } })
    expect(screen.getByText(/a\.tex, b\.tex/)).toBeTruthy()
  })

  it('loads the build log when the Log tab opens', async () => {
    api.getBuildLog.mockResolvedValue('This is XeTeX\n! Undefined control sequence.')
    const compile = compileWith({ buildId: 'b'.repeat(32) })
    render(ProblemsPanel, { props: { compile } })

    await fireEvent.click(screen.getByRole('tab', { name: 'Log' }))
    await waitFor(() => expect(screen.getByText(/This is XeTeX/)).toBeTruthy())
    expect(api.getBuildLog).toHaveBeenCalledWith('b'.repeat(32))
  })

  it('reports a log that cannot be loaded, and no log before a build', async () => {
    api.getBuildLog.mockRejectedValue(new Error('pruned'))
    const { unmount } = render(ProblemsPanel, { props: { compile: compileWith({ buildId: null }) } })
    await fireEvent.click(screen.getByRole('tab', { name: 'Log' }))
    expect(screen.getByText(/No build yet/)).toBeTruthy()
    unmount()

    render(ProblemsPanel, { props: { compile: compileWith({ buildId: 'c'.repeat(32) }) } })
    await fireEvent.click(screen.getByRole('tab', { name: 'Log' }))
    await waitFor(() => expect(screen.getByText(/pruned/)).toBeTruthy())
  })
})

describe('ConflictDialog', () => {
  it('offers both ways forward and a way to postpone, and decides nothing itself', async () => {
    const onresolve = vi.fn()
    const ondismiss = vi.fn()
    render(ConflictDialog, { props: { doc: { path: 'main.tex' }, onresolve, ondismiss } })

    expect(screen.getByRole('alertdialog').textContent).toContain('main.tex changed on disk')
    await fireEvent.click(screen.getByRole('button', { name: 'Keep my version' }))
    await fireEvent.click(screen.getByRole('button', { name: 'Load the file from disk' }))
    await fireEvent.click(screen.getByRole('button', { name: 'Decide later' }))
    expect(onresolve.mock.calls).toEqual([['overwrite'], ['reload']])
    expect(ondismiss).toHaveBeenCalledTimes(1)
  })
})

describe('CommandMenu', () => {
  function registry() {
    const r = new CommandRegistry()
    const calls = []
    r.register({ id: 'save', title: 'Save file', keys: 'Mod+S', run: () => calls.push('save') })
    r.register({ id: 'compile', title: 'Compile', keys: 'Mod+Enter', run: () => calls.push('compile') })
    r.register({ id: 'stop', title: 'Stop compiling', run: () => calls.push('stop'), enabled: () => false })
    return { r, calls }
  }

  it('lists commands with their shortcuts and filters as you type', async () => {
    const { r } = registry()
    render(CommandMenu, { props: { commands: r } })
    expect(screen.getAllByRole('option')).toHaveLength(3)
    expect(screen.getByText('Save file').closest('button').textContent).toMatch(/Ctrl\+S|⌘S/)

    await fireEvent.input(screen.getByLabelText('Search commands'), { target: { value: 'comp' } })
    expect(screen.getAllByRole('option').map((o) => o.textContent.trim())).toEqual([
      expect.stringContaining('Compile'),
      expect.stringContaining('Stop compiling'),
    ])
  })

  it('Enter runs the highlighted command and closes; a disabled one does not run', async () => {
    const { r, calls } = registry()
    const onclose = vi.fn()
    render(CommandMenu, { props: { commands: r, onclose } })
    const input = screen.getByLabelText('Search commands')

    await fireEvent.keyDown(input, { key: 'ArrowDown' })
    await fireEvent.keyDown(input, { key: 'Enter' })
    await waitFor(() => expect(calls).toEqual(['compile']))
    expect(onclose).toHaveBeenCalled()

    await fireEvent.input(input, { target: { value: 'stop' } })
    await fireEvent.keyDown(input, { key: 'Enter' })
    expect(calls).toEqual(['compile'])
  })

  it('Escape closes', async () => {
    const { r } = registry()
    const onclose = vi.fn()
    render(CommandMenu, { props: { commands: r, onclose } })
    await fireEvent.keyDown(screen.getByLabelText('Search commands'), { key: 'Escape' })
    expect(onclose).toHaveBeenCalled()
  })
})

describe('PdfPane', () => {
  const compileWith = (over = {}) => {
    const c = new CompileState({ api: {} })
    Object.assign(c, over)
    return c
  }
  const built = (over = {}) => compileWith({ status: 'success', buildId: 'a'.repeat(32), pageCount: 3, mainFile: 'main.tex', ...over })

  beforeEach(() => localStorage.clear())

  it('has one slot per page, and every page URL names the build', () => {
    render(PdfPane, { props: { compile: built(), sourcePath: 'main.tex' } })
    const slots = document.querySelectorAll('[data-page]')
    expect(slots).toHaveLength(3)
    const srcs = [...document.querySelectorAll('.slot img')].map((i) => i.getAttribute('src'))
    expect(srcs).toHaveLength(3) // jsdom has no IntersectionObserver, so all load
    for (const [i, src] of srcs.entries()) {
      expect(src).toContain(`build=${'a'.repeat(32)}`)
      expect(src).toContain(`n=${i + 1}`)
    }
  })

  it('links the finished PDF for export', () => {
    render(PdfPane, { props: { compile: built(), sourcePath: 'main.tex' } })
    expect(screen.getByRole('link', { name: 'Export PDF' }).getAttribute('href')).toBe(`/export/${'a'.repeat(32)}`)
  })

  it('zooms in steps and remembers the choice', async () => {
    render(PdfPane, { props: { compile: built(), sourcePath: 'main.tex' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Zoom in' }))
    expect(screen.getByText('125%')).toBeTruthy()
    await fireEvent.click(screen.getByRole('button', { name: 'Zoom out' }))
    await fireEvent.click(screen.getByRole('button', { name: 'Zoom out' }))
    expect(screen.getByText('75%')).toBeTruthy()
    expect(JSON.parse(localStorage.getItem('latex-ai:zoom'))).toBe(0.75)
  })

  it('page controls are disabled before there is a PDF, and a Compile button invites one', async () => {
    const oncompile = vi.fn()
    render(PdfPane, { props: { compile: compileWith(), sourcePath: 'main.tex', oncompile } })
    expect(screen.getByRole('button', { name: 'Zoom in' }).disabled).toBe(true)
    expect(screen.getByText('No PDF yet')).toBeTruthy()
    await fireEvent.click(screen.getByRole('button', { name: 'Compile' }))
    expect(oncompile).toHaveBeenCalled()
  })

  it('after a failed compile it says there is no PDF rather than showing a stale one', () => {
    render(PdfPane, { props: { compile: compileWith({ status: 'error' }), sourcePath: 'main.tex' } })
    expect(screen.getByText(/last compile failed/)).toBeTruthy()
  })
})

describe('Toolbar', () => {
  function setup({ active = { path: 'main.tex' }, candidates = [], main = null } = {}) {
    localStorage.clear()
    const ui = new UiState()
    const compile = new CompileState({ api: {} })
    compile.candidates = candidates
    const project = projectWith()
    project.mainOverride = main
    const commands = new CommandRegistry()
    commands.register({ id: 'compile', title: 'Compile', keys: 'Mod+Enter', run() {} })
    const documents = { active }
    const handlers = { oncompile: vi.fn(), oncancel: vi.fn(), onsettings: vi.fn(), onhistory: vi.fn(), onpalette: vi.fn() }
    render(Toolbar, { props: { ui, documents, compile, project, commands, ...handlers } })
    return { ui, compile, project, ...handlers }
  }

  it('Compile needs an open file, shows its shortcut, and runs', async () => {
    const { oncompile } = setup()
    const button = screen.getByRole('button', { name: 'Compile' })
    expect(button.getAttribute('title')).toMatch(/Ctrl\+Enter|⌘Enter/)
    await fireEvent.click(button)
    expect(oncompile).toHaveBeenCalled()
  })

  it('is disabled with nothing open', () => {
    setup({ active: null })
    expect(screen.getByRole('button', { name: 'Compile' }).disabled).toBe(true)
  })

  it('offers a Stop button only while compiling', async () => {
    const { compile, oncancel } = setup()
    expect(screen.queryByRole('button', { name: 'Stop' })).toBeNull()
    compile.status = 'compiling'
    await waitFor(() => expect(screen.getByRole('button', { name: 'Stop' })).toBeTruthy())
    await fireEvent.click(screen.getByRole('button', { name: 'Stop' }))
    expect(oncancel).toHaveBeenCalled()
  })

  it('shows the main-document chooser only when there is a choice to make', () => {
    setup()
    expect(screen.queryByLabelText('Main document')).toBeNull()
  })

  it('shows it, with the candidates, when the project could not decide', () => {
    setup({ candidates: ['a.tex', 'b.tex'] })
    const select = screen.getByLabelText('Main document')
    expect([...select.options].map((o) => o.value)).toEqual(['', 'a.tex', 'b.tex'])
  })

  it('choosing a main document is remembered', async () => {
    const { project } = setup({ candidates: ['a.tex', 'b.tex'] })
    await fireEvent.change(screen.getByLabelText('Main document'), { target: { value: 'b.tex' } })
    expect(project.mainOverride).toBe('b.tex')
  })

  it('shows the project switcher only where the shell can switch (desktop)', async () => {
    localStorage.clear()
    const mk = (extra) => {
      const project = projectWith()
      project.name = 'thesis'
      project.root = '/home/me/thesis'
      const commands = new CommandRegistry()
      return render(Toolbar, { props: { ui: new UiState(), documents: { active: null }, compile: new CompileState({ api: {} }), project, commands, ...extra } })
    }
    const { unmount } = mk({})
    expect(screen.queryByRole('button', { name: /thesis/ })).toBeNull()
    unmount()

    const onopenproject = vi.fn()
    mk({ onopenproject })
    const button = screen.getByRole('button', { name: /thesis/ })
    expect(button.getAttribute('title')).toContain('/home/me/thesis')
    await fireEvent.click(button)
    expect(onopenproject).toHaveBeenCalled()
  })

  it('panes toggle from the toolbar', async () => {
    const { ui } = setup()
    await fireEvent.click(screen.getByRole('button', { name: 'Assistant' }))
    expect(ui.chat).toBe(false)
  })
})

describe('OutlinePanel', () => {
  it('lists headings, indents by depth, and jumps to the line', async () => {
    const { default: OutlinePanel } = await import('../src/components/OutlinePanel.svelte')
    const model = { onDidChangeContent: vi.fn(() => ({ dispose() {} })) }
    const doc = { path: 'main.tex', model, text: '\\section{One}\n\\subsection{Two}\n\\section*{Three}' }
    const onreveal = vi.fn()
    render(OutlinePanel, { props: { documents: { active: doc }, onreveal } })

    const items = await screen.findAllByRole('button', { name: /One|Two|Three/ })
    expect(items.map((b) => b.textContent.trim())).toEqual(['One', 'Two', 'Three'])
    expect(parseFloat(items[1].style.paddingLeft)).toBeGreaterThan(parseFloat(items[0].style.paddingLeft))
    await fireEvent.click(items[1])
    expect(onreveal).toHaveBeenCalledWith(2)
  })

  it('says so for a file without headings and ignores non-LaTeX files', async () => {
    const { default: OutlinePanel } = await import('../src/components/OutlinePanel.svelte')
    const model = { onDidChangeContent: vi.fn(() => ({ dispose() {} })) }
    render(OutlinePanel, { props: { documents: { active: { path: 'refs.bib', model, text: '\\section{x}' } } } })
    expect(await screen.findByText(/No headings/)).toBeTruthy()
  })
})
