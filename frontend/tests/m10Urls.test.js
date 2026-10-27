import { describe, it, expect } from 'vitest'
import { APIClient } from '../src/lib/api/client.js'

describe('build-addressed URLs', () => {
  const client = new APIClient()

  it('a page URL names the build when there is one', () => {
    const url = client.pdfPageUrl('chapters/a.tex', 2, 150, undefined, 'abc123')
    expect(url).toContain('build=abc123')
    expect(url).not.toContain('file=')
    expect(url).toContain('n=2')
  })

  it('without a build it falls back to "newest build of this file"', () => {
    const url = client.pdfPageUrl('main.tex', 1)
    expect(url).toContain('file=main.tex')
    expect(url).not.toContain('build=')
  })

  it('export and build lookups are encoded', () => {
    expect(client.exportPdfUrl('a/b')).toMatch(/\/builds\/a%2Fb\/pdf$/)
  })
})
