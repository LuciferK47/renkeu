import { describe, it, expect } from 'vitest';
import * as copy from '../src/landing/copy.js';

const text = JSON.stringify(copy).toLowerCase();

describe('landing copy tells the truth', () => {
  it('avoids words that over-promise', () => {
    for (const w of ['autonomous', 'revolutionize', 'supercharge', 'seamless', 'next-generation', 'magic']) {
      expect(text).not.toContain(w);
    }
  });

  it('does not claim features that are not built', () => {
    const shipped = JSON.stringify({ ...copy.capabilities, planned: undefined, loop: copy.loop, hero: copy.hero }).toLowerCase();
    expect(shipped).not.toContain('collaborat');
    expect(shipped).not.toContain('8-bit');
    expect(shipped).not.toContain('page limit');
    expect(shipped).not.toContain('mcp');
  });

  it('lists the planned features as planned', () => {
    expect(copy.capabilities.planned.items.join(' ').toLowerCase()).toContain('8-bit');
  });

  it('states the real attempt limit and the real data boundary', () => {
    expect(copy.loop.footnote).toContain('3 attempts');
    expect(copy.boundary.sent.join(' ')).toContain('up to 3 other files');
    expect(copy.boundary.sent.join(' ')).toContain('names of your other files');
  });

  it('does not offer a release that does not exist', () => {
    const desktop = copy.install.items.find((i) => i.id === 'desktop');
    expect(desktop.note.toLowerCase()).toContain('coming');
  });

  it('has five crew with a role each, and a sprite for every one', async () => {
    const { CREW_IDS, crewFrame } = await import('../src/lib/sprites/crew.js')
    expect(Object.keys(copy.crew).filter((k) => k !== 'repair').sort()).toEqual([...CREW_IDS].sort())
    for (const id of CREW_IDS) {
      const a = crewFrame(id, 0)
      const b = crewFrame(id, 1)
      expect(new Set(a.map((r) => r.length)).size, id).toBe(1)
      expect(a.length).toBe(14)
      expect(b, id).not.toEqual(a)
    }
  })

  it('names exactly the six crates and one label per action', () => {
    expect(copy.engine.crates.map((c) => c.name)).toEqual(['compiler', 'latex_ai_core', 'latex_ai_server', 'synctex', 'visual', 'latex_ai_tauri']);
    expect(Object.keys(copy.actions)).toEqual(['get', 'open', 'source']);
  });
});
