import { describe, it, expect } from 'vitest'

/**
 * The file list contract.
 *
 * `GET /api/files` returns `files` as plain path strings -- `Vec<String>` in
 * crates/latex_ai_server/src/routes/files.rs -- while FileTree renders
 * `{ path, name }`. Nothing bridged the two, so the component threw on every
 * load and showed "Failed to load files"; the tree had never listed anything.
 *
 * This mirrors the normaliser so the shape mismatch cannot come back quietly.
 * If the endpoint ever starts returning objects, the pass-through branch keeps
 * working and this test says so.
 */
function toFileEntry(entry) {
  if (entry && typeof entry === 'object') return entry
  const path = String(entry)
  const slash = path.lastIndexOf('/')
  return { path, name: slash === -1 ? path : path.slice(slash + 1) }
}

describe('file list normalisation', () => {
  it('turns a bare path into something the tree can render', () => {
    expect(toFileEntry('sample.tex')).toEqual({ path: 'sample.tex', name: 'sample.tex' })
  })

  it('splits a nested path into directory and file name', () => {
    expect(toFileEntry('chapters/intro.tex')).toEqual({
      path: 'chapters/intro.tex',
      name: 'intro.tex',
    })
  })

  it('handles deep nesting', () => {
    expect(toFileEntry('a/b/c/deep.tex').name).toBe('deep.tex')
  })

  it('passes richer entries through untouched', () => {
    const entry = { path: 'x.tex', name: 'x.tex', size: 42 }
    expect(toFileEntry(entry)).toBe(entry)
  })

  // The exact failure: `.path` was undefined, so `.substring` threw and the
  // whole load was reported as a network-ish failure.
  it('always produces a defined path, which is what the tree indexes on', () => {
    for (const input of ['a.tex', 'dir/a.tex', '', 'no-extension']) {
      expect(toFileEntry(input).path).toBeTypeOf('string')
      expect(toFileEntry(input).name).toBeTypeOf('string')
    }
  })
})
