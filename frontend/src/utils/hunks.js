/**
 * Selection logic for per-hunk review (masterplan §12 M4).
 *
 * Kept out of the component so it can be tested directly: these functions
 * decide what actually gets written to the user's file.
 */

/**
 * Merge proposed edits with their server-computed previews.
 * Order follows the previews, which is the order the server would apply in.
 */
export function mergePreviews(edits, hunks) {
  return hunks.map((hunk) => ({
    ...hunk,
    edit: edits.find((e) => e.hunk_id === hunk.hunk_id) ?? null
  }));
}

/** Hunks that still match the file, i.e. the only ones safe to select. */
export function applicable(previews) {
  return previews.filter((p) => p.applies);
}

/**
 * Default selection for a fresh proposal: everything that applies.
 * The common case is "yes, all of this"; rejecting is one click.
 */
export function defaultSelection(previews) {
  return new Set(applicable(previews).map((p) => p.hunk_id));
}

/** Toggle one hunk, returning a new Set (never mutate reactive state). */
export function toggleHunk(selected, hunkId) {
  const next = new Set(selected);
  if (next.has(hunkId)) next.delete(hunkId);
  else next.add(hunkId);
  return next;
}

/** True when every applicable hunk is selected (and there is at least one). */
export function isAllSelected(previews, selected) {
  const usable = applicable(previews);
  return usable.length > 0 && usable.every((p) => selected.has(p.hunk_id));
}

/** Select-all / select-none, based on where we currently are. */
export function toggleAll(previews, selected) {
  return isAllSelected(previews, selected) ? new Set() : defaultSelection(previews);
}

/**
 * The ids to send to apply-selected: selected AND still applicable.
 * A stale hunk in the payload would abort the whole batch server-side, so it
 * is filtered here even if it somehow got selected.
 */
export function selectedHunkIds(previews, selected) {
  return applicable(previews)
    .filter((p) => selected.has(p.hunk_id))
    .map((p) => p.hunk_id);
}
