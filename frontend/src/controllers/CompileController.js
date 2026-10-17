import * as compileService from '../services/compileService.js';
import { api } from '../lib/api/client.js';
import { compileStore } from '../stores/compileStore.js';
import { uiStore, pushToast } from '../stores/uiStore.js';

let compileController = null;
let compileQueued = false;
let compiling = false;

compileStore.subscribe((s) => (compiling = s.status === 'compiling'));

/** Split the server's diagnostics into the error/warning lists the panel shows. */
export function splitDiagnostics(diagnostics) {
  const list = Array.isArray(diagnostics) ? diagnostics : [];
  const shape = (d) => ({
    message: d.message,
    file: d.file ?? null,
    line: d.line ?? null,
    context: d.context ?? null,
    kind: d.kind
  });
  return {
    errors: list.filter((d) => d.severity === 'error').map(shape),
    warnings: list.filter((d) => d.severity === 'warning').map(shape)
  };
}

/** Stop the running compile, if any. The server kills the worker process. */
export async function cancelCompile() {
  let jobId = null;
  compileStore.subscribe((c) => (jobId = c.status === 'compiling' ? c.jobId : null))();
  if (!jobId) return false;
  try {
    await api.cancelCompile(jobId);
    return true;
  } catch (e) {
    pushToast(`Could not cancel: ${e.message}`, 'error');
    return false;
  }
}

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
          const { errors, warnings } = splitDiagnostics(data.diagnostics);
          if (data.success) {
            compileStore.update((c) => ({
              ...c,
              status: 'success',
              progress: 100,
              pageCount: data.page_count ?? 1,
              buildId: data.build_id ?? null,
              mainFile: data.main_file ?? null,
              pdfUrl: compileService.pdfPageUrl(filePath, 1, undefined, data.build_id),
              errors: [],
              warnings,
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
              buildId: data.build_id ?? null,
              mainFile: data.main_file ?? null,
              // Located problems when the server found them (file and line),
              // else the one-line summary.
              errors: errors.length > 0 ? errors : [{ message }],
              warnings,
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
