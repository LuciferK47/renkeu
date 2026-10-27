import { describe, it, expect } from 'vitest'
import { lineRange, lineText, singleLine } from '../src/lib/quickedit.js'

const text = 'one\ntwo words\r\n\nlast'

describe('lineRange', () => {
  it('gives the offsets of a line without its ending', () => {
    expect(lineRange(text, 1)).toEqual({ start: 0, end: 3 })
    const second = lineRange(text, 2)
    expect(text.slice(second.start, second.end)).toBe('two words')
    expect(lineRange(text, 3)).toEqual({ start: 15, end: 15 })
    expect(text.slice(lineRange(text, 4).start)).toBe('last')
  })
  it('is null past the end and for nonsense line numbers', () => {
    expect(lineRange(text, 5)).toBeNull()
    expect(lineRange(text, 0)).toBeNull()
    expect(lineRange(text, 1.5)).toBeNull()
    expect(lineRange('', 2)).toBeNull()
  })
  it('knows a file with no trailing newline and an empty file', () => {
    expect(lineRange('', 1)).toEqual({ start: 0, end: 0 })
    expect(lineText('a\nb', 2)).toBe('b')
  })
})

describe('singleLine', () => {
  it('turns line breaks into spaces so one line stays one line', () => {
    expect(singleLine('a\nb\r\nc')).toBe('a b c')
  })
})
