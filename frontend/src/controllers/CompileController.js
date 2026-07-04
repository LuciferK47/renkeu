import * as compileService from '../services/compileService.js';
import { compileStore, setCompileJob } from '../stores/compileStore.js';
import { uiStore, pushToast } from '../stores/uiStore.js';

let compileController = null;
let compileQueued = false;
let compiling = false;

compileStore.subscribe(s => compiling = s.status === 'compiling');

export async function compileDocument(filePath) {
  if (!filePath) {
    uiStore.update(ui => ({ ...ui, status: 'Please select a file first' }));
    return;
  }

  if (compiling) {
    compileQueued = true;
    return;
  }

  compileQueued = false;
  compileController?.abort();
  compileController = new AbortController();

  compileStore.update(c => ({
    ...c,
    status: 'compiling',
    errors: [],
    warnings: [],
    progress: 0
  }));
  
  uiStore.update(ui => ({
    ...ui,
    status: 'Submitting compilation job...',
    activeRightTab: 'pdf'
  }));
  pushToast('Compilation started', 'info');

  try {
    const data = await compileService.startCompilation(filePath, 'pdflatex', compileController.signal);
    
    compileStore.update(c => ({
      ...c,
      jobId: data.job_id,
      status: 'pending'
    }));
    uiStore.update(ui => ({ ...ui, status: 'Compilation queued...' }));
    
  } catch (e) {
    if (e?.name === 'AbortError') {
      uiStore.update(ui => ({ ...ui, status: 'Compilation superseded by newer request' }));
    } else {
      uiStore.update(ui => ({ ...ui, status: `Compilation error: ${e.message}`, activeRightTab: 'errors' }));
      compileStore.update(c => ({
        ...c,
        status: 'error',
        errors: [{ message: e.message }]
      }));
    }
  } finally {
    compileController = null;
    if (compileQueued) {
      compileQueued = false;
      compileDocument(filePath);
    }
  }
}
