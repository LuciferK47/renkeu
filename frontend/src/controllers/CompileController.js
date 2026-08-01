import * as compileService from '../services/compileService.js';
import { compileStore } from '../stores/compileStore.js';
import { uiStore, pushToast } from '../stores/uiStore.js';

let compileController = null;
let compileQueued = false;
let compiling = false;

compileStore.subscribe((s) => (compiling = s.status === 'compiling'));

/**
 * Start a compile job and follow it to completion over SSE.
 * The backend job outlives this request, so navigating away does not kill it.
 */
export async function compileDocument(filePath) {
  if (!filePath) {
    uiStore.update((ui) => ({ ...ui, status: 'Please select a file first' }));
    return;
  }

  // Coalesce: one in-flight compile, one queued behind it.
  if (compiling) {
    compileQueued = true;
    return;
  }
  compileQueued = false;
  compileController?.abort();
  compileController = new AbortController();

  compileStore.update((c) => ({
    ...c,
    status: 'compiling',
    errors: [],
    warnings: [],
    progress: 0
  }));
  uiStore.update((ui) => ({ ...ui, status: 'Compiling...', activeRightTab: 'pdf' }));

  try {
    const { job_id: jobId } = await compileService.startCompilation(
      filePath,
      compileController.signal
    );
    compileStore.update((c) => ({ ...c, jobId, status: 'compiling' }));

    await compileService.streamCompilationStatus(
      jobId,
      ({ event, data }) => {
        if (event === 'progress') {
          compileStore.update((c) => ({
            ...c,
            progress: Math.round((data.progress ?? 0) * 100)
          }));
          uiStore.update((ui) => ({ ...ui, status: data.message ?? 'Compiling...' }));
        } else if (event === 'compiled') {
          if (data.success) {
            compileStore.update((c) => ({
              ...c,
              status: 'success',
              progress: 100,
              pageCount: data.page_count ?? 1,
              pdfUrl: compileService.pdfPageUrl(filePath, 1),
              errors: [],
              lastCompileAt: new Date()
            }));
            uiStore.update((ui) => ({ ...ui, status: 'Compiled' }));
            pushToast('Compiled successfully', 'success');
          } else {
            const message = data.error_summary ?? 'Compilation failed';
            compileStore.update((c) => ({
              ...c,
              status: 'error',
              progress: 0,
              errors: [{ message }],
              lastCompileAt: new Date()
            }));
            uiStore.update((ui) => ({ ...ui, status: 'Compile failed', activeRightTab: 'errors' }));
            pushToast('Compile failed', 'error');
          }
        } else if (event === 'error') {
          compileStore.update((c) => ({
            ...c,
            status: 'error',
            errors: [{ message: data.message }]
          }));
        }
      },
      (error) => {
        compileStore.update((c) => ({
          ...c,
          status: 'error',
          errors: [{ message: error.message }]
        }));
        uiStore.update((ui) => ({ ...ui, status: `Compile error: ${error.message}` }));
      }
    );
  } catch (e) {
    if (e?.name === 'AbortError') {
      uiStore.update((ui) => ({ ...ui, status: 'Compilation superseded by newer request' }));
    } else {
      uiStore.update((ui) => ({
        ...ui,
        status: `Compilation error: ${e.message}`,
        activeRightTab: 'errors'
      }));
      compileStore.update((c) => ({ ...c, status: 'error', errors: [{ message: e.message }] }));
    }
  } finally {
    compileController = null;
    if (compileQueued) {
      compileQueued = false;
      compileDocument(filePath);
    }
  }
}
