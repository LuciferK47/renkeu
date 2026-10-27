import { describe, it, expect } from 'vitest'
import {
  escapeLatex,
  labelFrom,
  uniqueLabel,
  figureBlock,
  tableBlock,
  parseGrid,
  equationBlock,
  citeCommand,
  refCommand,
  packageForCommand,
  packageEdit,
  packageFixFor,
  blockInsertion,
  cleanBibtex
} from '../src/lib/latex/insert.js'

describe('escapeLatex', () => {
  it('escapes every special character', () => {
    expect(escapeLatex('a & b % c $ d # e _ f { g } h ~ i ^ j \\ k')).toBe(
      'a \\& b \\% c \\$ d \\# e \\_ f \\{ g \\} h \\textasciitilde{} i \\textasciicircum{} j \\textbackslash{} k'
    )
  })
})

describe('labels', () => {
  it('makes a readable, safe label from text', () => {
    expect(labelFrom('fig', 'Results: the Naïve Method!')).toBe('fig:results-the-naive-method')
    expect(labelFrom('tab', '   ')).toBe('tab:untitled')
    expect(labelFrom('fig', 'x'.repeat(100)).length).toBeLessThanOrEqual(36)
  })
  it('avoids a label that is already taken', () => {
    expect(uniqueLabel('fig:a', ['fig:b'])).toBe('fig:a')
    expect(uniqueLabel('fig:a', ['fig:a', 'fig:a-2'])).toBe('fig:a-3')
  })
})

describe('figureBlock', () => {
  it('writes a centred figure with caption and label', () => {
    expect(figureBlock({ path: 'figures/plot.png', caption: 'Speed & cost', label: 'fig:speed', widthPct: 80 })).toBe(
      [
        '\\begin{figure}[htbp]',
        '  \\centering',
        '  \\includegraphics[width=0.8\\linewidth]{figures/plot.png}',
        '  \\caption{Speed \\& cost}',
        '  \\label{fig:speed}',
        '\\end{figure}'
      ].join('\n')
    )
  })
  it('clamps the width and ignores unknown placement letters', () => {
    const t = figureBlock({ path: 'a.png', widthPct: 500, placement: 'tx!' })
    expect(t).toContain('[t!]')
    expect(t).toContain('width=1\\linewidth')
    expect(figureBlock({ path: 'a.png', widthPct: 1 })).toContain('width=0.05\\linewidth')
  })
  it('leaves out an empty caption and label', () => {
    const t = figureBlock({ path: 'a.png' })
    expect(t).not.toContain('\\caption')
    expect(t).not.toContain('\\label')
  })
})

describe('tableBlock', () => {
  const rows = [['Case', 'Time (s)'], ['A & B', '1.2'], ['C', '50%']]
  it('writes booktabs rules, a header rule and escaped cells', () => {
    expect(tableBlock({ rows, align: ['l', 'r'], caption: 'Timings', label: 'tab:t' })).toBe(
      [
        '\\begin{table}[htbp]',
        '  \\centering',
        '  \\caption{Timings}',
        '  \\label{tab:t}',
        '  \\begin{tabular}{lr}',
        '    \\toprule',
        '    Case & Time (s) \\\\',
        '    \\midrule',
        '    A \\& B & 1.2 \\\\',
        '    C & 50\\% \\\\',
        '    \\bottomrule',
        '  \\end{tabular}',
        '\\end{table}'
      ].join('\n')
    )
  })
  it('can skip the header rule and keep raw LaTeX cells', () => {
    const t = tableBlock({ rows: [['$x^2$', 'b']], header: false, rawCells: true })
    expect(t).toContain('$x^2$ & b \\\\')
    expect(t).not.toContain('\\midrule')
  })
  it('pads ragged rows and defaults unknown alignment to left', () => {
    const t = tableBlock({ rows: [['a', 'b', 'c'], ['d']], align: ['z', 'c'] })
    expect(t).toContain('\\begin{tabular}{lcl}')
    expect(t).toContain('d &  &  \\\\')
  })
})

describe('parseGrid', () => {
  it('reads tab-separated spreadsheet text, or commas', () => {
    expect(parseGrid('a\tb\n1\t2\n')).toEqual([['a', 'b'], ['1', '2']])
    expect(parseGrid('a, b\r\n1, 2')).toEqual([['a', 'b'], ['1', '2']])
    expect(parseGrid('')).toEqual([])
  })
})

describe('equationBlock', () => {
  it('writes inline, display and numbered maths', () => {
    expect(equationBlock({ latex: ' x^2 ', mode: 'inline' })).toBe('$x^2$')
    expect(equationBlock({ latex: 'x^2' })).toBe('\\[\n  x^2\n\\]')
    expect(equationBlock({ latex: 'x^2', mode: 'numbered', label: 'eq:sq' })).toBe(
      '\\begin{equation}\n  \\label{eq:sq}\n  x^2\n\\end{equation}'
    )
  })
})

describe('citations and references', () => {
  it('joins keys and names the command', () => {
    expect(citeCommand('cite', ['a', 'b'])).toBe('\\cite{a,b}')
    expect(refCommand('cref', 'fig:x')).toBe('\\cref{fig:x}')
  })
})

describe('packages', () => {
  it('knows which package defines common commands', () => {
    expect(packageForCommand('toprule')).toBe('booktabs')
    expect(packageForCommand('includegraphics')).toBe('graphicx')
    expect(packageForCommand('nothing')).toBeNull()
  })

  it('adds only the missing packages, before \\begin{document}', () => {
    const src = '\\documentclass{article}\n\\usepackage{graphicx}\n\\begin{document}\nx\n\\end{document}\n'
    const edit = packageEdit(src, ['graphicx', 'booktabs'])
    expect(edit.text).toBe('\\usepackage{booktabs}\n')
    expect(src.slice(edit.offset)).toMatch(/^\\begin\{document\}/)
    expect(packageEdit(src, ['graphicx'])).toBeNull()
  })

  it('falls back to just after \\documentclass, and gives up without one', () => {
    const edit = packageEdit('\\documentclass[11pt]{article}\nHello\n', ['booktabs'])
    expect('\\documentclass[11pt]{article}\nHello\n'.slice(0, edit.offset)).toBe('\\documentclass[11pt]{article}\n')
    expect(packageEdit('Hello', ['booktabs'])).toBeNull()
  })

  it('sees packages loaded together in one command', () => {
    expect(packageEdit('\\documentclass{article}\n\\usepackage{amsmath,booktabs}\n\\begin{document}\\end{document}', ['booktabs'])).toBeNull()
  })
})

describe('cleanBibtex', () => {
  const crossref = '@article{Knuth_1984, title={Literate Programming}, volume={27}, ISSN={1460-2067}, author={Knuth, D. E.}, year={1984}}'
  it('gives a readable key and changes nothing else', () => {
    const { key, text } = cleanBibtex(crossref)
    expect(key).toBe('knuth1984literate')
    expect(text.startsWith('@article{knuth1984literate, title={Literate Programming}')).toBe(true)
    expect(text.endsWith('}\n')).toBe(true)
  })
  it('finds the surname in "First Last" form and skips stop words', () => {
    const { key } = cleanBibtex('@inproceedings{x, title={The Art of Computer Programming}, author={Donald Knuth and Other One}, year={1968}}')
    expect(key).toBe('knuth1968computer')
  })
  it('keeps the key unique among those already in the project', () => {
    expect(cleanBibtex(crossref, ['knuth1984literate']).key).toBe('knuth1984literatea')
    expect(cleanBibtex(crossref, ['knuth1984literate', 'knuth1984literatea']).key).toBe('knuth1984literateb')
  })
  it('returns the text untouched when there is no entry', () => {
    expect(cleanBibtex('nope')).toEqual({ key: '', text: 'nope' })
  })
})

describe('packageFixFor', () => {
  const text = '\\documentclass{article}\n\\begin{document}\n\\toprule x\n\\end{document}\n'
  it('offers the package for an undefined command it knows', () => {
    const fix = packageFixFor({ message: 'Undefined control sequence.', line: '\\toprule x', text })
    expect(fix.package).toBe('booktabs')
    expect(fix.command).toBe('toprule')
    expect(fix.edit.text).toBe('\\usepackage{booktabs}\n')
  })
  it('stays quiet for other errors, unknown commands and packages already loaded', () => {
    expect(packageFixFor({ message: 'Missing $ inserted.', line: '\\toprule', text })).toBeNull()
    expect(packageFixFor({ message: 'Undefined control sequence.', line: '\\mystery', text })).toBeNull()
    const loaded = text.replace('\\begin{document}', '\\usepackage{booktabs}\n\\begin{document}')
    expect(packageFixFor({ message: 'Undefined control sequence.', line: '\\toprule', text: loaded })).toBeNull()
  })
  it('skips a known command that is loaded and finds the next one on the line', () => {
    const t = text.replace('\\begin{document}', '\\usepackage{graphicx}\n\\begin{document}')
    const fix = packageFixFor({ message: 'Undefined control sequence.', line: '\\includegraphics \\toprule', text: t })
    expect(fix.package).toBe('booktabs')
  })
})

describe('blockInsertion', () => {
  const doc = 'First line.\n\nSecond paragraph here.\nMore.\n'
  const apply = (offset, block = 'BLOCK') => {
    const at = blockInsertion(doc, offset, block)
    return doc.slice(0, at.offset) + at.text + doc.slice(at.offset)
  }
  it('puts a block on a blank line between blank lines', () => {
    expect(apply(12)).toBe('First line.\n\nBLOCK\n\nSecond paragraph here.\nMore.\n')
  })
  it('never splits a line: a cursor inside text puts the block after the line', () => {
    expect(apply(20)).toBe('First line.\n\nSecond paragraph here.\n\nBLOCK\n\nMore.\n')
  })
  it('puts the block above a line when the cursor is at its start, with blank lines around it', () => {
    expect(apply(13)).toBe('First line.\n\nBLOCK\n\nSecond paragraph here.\nMore.\n')
  })
  it('handles the very start and the very end of the document', () => {
    expect(blockInsertion('', 0, 'B')).toEqual({ offset: 0, text: 'B\n' })
    const end = 'last'
    const at = blockInsertion(end, 4, 'B')
    expect(end.slice(0, at.offset) + at.text + end.slice(at.offset)).toBe('last\n\nB')
  })
  it('two blocks inserted one after the other stay apart', () => {
    let text = 'a\n\nb\n'
    let offset = 3 // the start of "b"
    let at = blockInsertion(text, offset, 'ONE')
    text = text.slice(0, at.offset) + at.text + text.slice(at.offset)
    offset = at.offset + at.text.length // the cursor ends up after the inserted text
    at = blockInsertion(text, offset, 'TWO')
    text = text.slice(0, at.offset) + at.text + text.slice(at.offset)
    expect(text).toContain('ONE\n\nTWO')
    expect(text.indexOf('ONE')).toBeLessThan(text.indexOf('TWO'))
  })
})
