// Per-hunk selection decides what gets written to the user's file, so the
// rules are pinned here rather than left to the component (masterplan §12 M4).
import { describe, it, expect } from 'vitest';
import {
  mergePreviews,
  applicable,
  defaultSelection,
  toggleHunk,
  isAllSelected,
  toggleAll,
  selectedHunkIds
} from '../src/utils/hunks.js';

const edits = [
  { hunk_id: 'aaa', op: 'replace_range', text: 'alpha' },
  { hunk_id: 'bbb', op: 'replace_range', text: 'beta' },
  { hunk_id: 'ccc', op: 'insert', insert_text: 'gamma' }
];

const previews = [
  { hunk_id: 'aaa', diff: '-alpha\n+ALPHA', applies: true, error: null },
  { hunk_id: 'bbb', diff: '', applies: false, error: 'search text not found' },
  { hunk_id: 'ccc', diff: '+gamma', applies: true, error: null }
];

describe('mergePreviews', () => {
  it('pairs each preview with its edit, keeping preview order', () => {
    const merged = mergePreviews(edits, previews);
    expect(merged.map((m) => m.hunk_id)).toEqual(['aaa', 'bbb', 'ccc']);
    expect(merged[0].edit.op).toBe('replace_range');
    expect(merged[2].edit.insert_text).toBe('gamma');
  });

  it('tolerates a preview with no matching edit', () => {
    const merged = mergePreviews([], previews);
    expect(merged[0].edit).toBeNull();
  });
});

describe('selection defaults', () => {
  it('selects every applicable hunk and no stale ones', () => {
    const selected = defaultSelection(previews);
    expect([...selected].sort()).toEqual(['aaa', 'ccc']);
    expect(selected.has('bbb')).toBe(false);
  });

  it('selects nothing when nothing applies', () => {
    expect(defaultSelection([{ hunk_id: 'x', applies: false }]).size).toBe(0);
  });

  it('reports which hunks are usable', () => {
    expect(applicable(previews).map((p) => p.hunk_id)).toEqual(['aaa', 'ccc']);
  });
});

describe('toggleHunk', () => {
  it('adds and removes without mutating the original set', () => {
    const original = new Set(['aaa']);
    const added = toggleHunk(original, 'ccc');
    expect([...added].sort()).toEqual(['aaa', 'ccc']);
    expect([...original]).toEqual(['aaa']);

    const removed = toggleHunk(added, 'aaa');
    expect([...removed]).toEqual(['ccc']);
  });
});

describe('select all', () => {
  it('is true only when every applicable hunk is selected', () => {
    expect(isAllSelected(previews, new Set(['aaa', 'ccc']))).toBe(true);
    expect(isAllSelected(previews, new Set(['aaa']))).toBe(false);
    // A stale hunk being unselected must not make "all" false.
    expect(isAllSelected(previews, new Set(['aaa', 'ccc', 'bbb']))).toBe(true);
  });

  it('is false when there is nothing to select', () => {
    expect(isAllSelected([], new Set())).toBe(false);
    expect(isAllSelected([{ hunk_id: 'x', applies: false }], new Set())).toBe(false);
  });

  it('toggles between all and none', () => {
    const none = new Set();
    const all = toggleAll(previews, none);
    expect([...all].sort()).toEqual(['aaa', 'ccc']);
    expect(toggleAll(previews, all).size).toBe(0);
  });
});

describe('selectedHunkIds', () => {
  it('sends only what the user chose', () => {
    expect(selectedHunkIds(previews, new Set(['aaa']))).toEqual(['aaa']);
  });

  it('never sends a stale hunk, which would abort the whole batch', () => {
    const ids = selectedHunkIds(previews, new Set(['aaa', 'bbb', 'ccc']));
    expect(ids).toEqual(['aaa', 'ccc']);
    expect(ids).not.toContain('bbb');
  });

  it('sends nothing when the user rejects everything', () => {
    expect(selectedHunkIds(previews, new Set())).toEqual([]);
  });
});
