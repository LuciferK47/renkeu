// Compile state, written by CompileController.
//
// This file used to export eight mutators -- setCompileJob, setCompileSuccess,
// setCompileError and friends -- with no callers anywhere in the repo. The
// controller writes the store directly instead, and because the helpers were
// never exercised their vocabulary drifted: `setCompileJob` defaulted to a
// `'pending'` status that nothing checks, while every consumer tests for
// `'compiling'`. Anyone who had reached for them would have found the compile
// button not disabling and the coalescing in CompileController not engaging.
//
// They are gone rather than tested, because a test over an unused mutator only
// locks in the wrong default. `resetCompile` survives: it is the one that
// carries real meaning, and it keeps a new document from inheriting the last
// one's errors.
import { writable } from 'svelte/store'

export const compileStore = writable({
  jobId: null,
  // The stored build the current PDF came from; page URLs name it.
  buildId: null,
  mainFile: null,
  // idle | compiling | success | error
  status: 'idle',
  progress: 0,
  pdfUrl: null,
  pageCount: 0,
  errors: [],
  warnings: [],
  lastCompileAt: null,
  // Set while a debounced auto-compile is waiting; StatusBar shows it.
  pendingAutoCompile: false,
})

/** Return to a clean slate, so a newly opened document shows no stale result. */
export function resetCompile() {
  compileStore.update((c) => ({
    ...c,
    jobId: null,
    buildId: null,
    mainFile: null,
    status: 'idle',
    progress: 0,
    pdfUrl: null,
    pageCount: 0,
    errors: [],
    warnings: [],
    lastCompileAt: null,
    pendingAutoCompile: false,
  }))
}
