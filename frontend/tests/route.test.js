import { describe, it, expect } from 'vitest';
import { initialView } from '../src/lib/state/route.svelte.js';

describe('initialView', () => {
  it('is the editor in the desktop shell, whatever the address says', () => {
    expect(initialView({ desktop: true, hash: '#/landing' })).toBe('editor');
  });
  it('shows the landing page on a first visit', () => {
    expect(initialView({})).toBe('landing');
  });
  it('goes straight to the editor once it has been opened before', () => {
    expect(initialView({ visited: true })).toBe('editor');
  });
  it('lets the address override the remembered choice', () => {
    expect(initialView({ visited: true, hash: '#/landing' })).toBe('landing');
    expect(initialView({ hash: '#/editor' })).toBe('editor');
    expect(initialView({ search: '?app' })).toBe('editor');
  });
});
