// Compile state and the one place that drives a compile job.
//
// The server job outlives the request that started it, so a closed tab does
// not kill a compile; this class follows it over SSE and can cancel it (which
// kills the worker process server-side).

import { api as defaultApi, APIError } from '../api/client.js';
import { streamCompilationStatus } from '../../services/compileService.js';
import { splitDiagnostics } from '../diagnostics.js';

export class CompileState {
  /** idle | compiling | success | error */
  status = $state('idle');
  progress = $state(0);
  message = $state('');
  jobId = $state(null);
  /** The stored build the current PDF came from; page URLs name it. */
  buildId = $state(null);
  mainFile = $state(null);
  pageCount = $state(0);
  errors = $state([]);
  warnings = $state([]);
  /** Root-document candidates when the project could not pick one. */
  candidates = $state([]);
  lastCompileAt = $state(null);

  #api;
  #stream;
  #abort = null;
  /** A request that arrived mid-compile; run once when it finishes. */
  #queued = null;

  constructor({ api = defaultApi, stream = streamCompilationStatus } = {}) {
    this.#api = api;
    this.#stream = stream;
  }

  get compiling() {
    return this.status === 'compiling';
  }

  /**
   * Compile the project that `activePath` belongs to. If a compile is already
   * running the request is remembered and run once afterwards, so a burst of
   * clicks costs one extra compile, not a pile of them.
   */
  async start(activePath, { main = null } = {}) {
    if (!activePath) return;
    if (this.compiling) {
      this.#queued = { activePath, main };
      return;
    }
    this.#queued = null;
    this.#abort = new AbortController();
    this.status = 'compiling';
    this.progress = 0;
    this.message = 'Compiling…';
    this.errors = [];
    this.warnings = [];
    this.candidates = [];

    try {
      const started = await this.#api.compile(activePath, this.#abort.signal, main);
      this.jobId = started.job_id;
      this.mainFile = started.main_file ?? this.mainFile;
      await this.#stream(
        started.job_id,
        ({ event, data }) => this.#onEvent(event, data),
        (error) => this.#fail(error.message)
      );
    } catch (e) {
      if (e?.name === 'AbortError') {
        this.status = 'idle';
        this.message = 'Compile superseded';
      } else if (e instanceof APIError && e.payload?.error === 'main_file_ambiguous') {
        this.candidates = e.payload.candidates ?? [];
        this.#fail('Several files could be the main document — choose one.');
      } else {
        this.#fail(e.message);
      }
    } finally {
      this.#abort = null;
      const next = this.#queued;
      this.#queued = null;
      if (next) await this.start(next.activePath, { main: next.main });
    }
  }

  /** Stop the running compile, if any. */
  async cancel() {
    if (!this.compiling || !this.jobId) return false;
    try {
      await this.#api.cancelCompile(this.jobId);
      return true;
    } catch (e) {
      this.message = `Could not cancel: ${e.message}`;
      return false;
    }
  }

  /**
   * Show the result of an earlier compile again (after a page reload) without
   * running one. Does nothing if a compile is running or has finished, or if
   * the build is gone.
   */
  async restore(buildId) {
    if (!buildId || this.status !== 'idle') return false;
    try {
      const build = await this.#api.getBuild(buildId);
      if (this.status !== 'idle') return false; // a compile started meanwhile
      const { errors, warnings } = splitDiagnostics(build.diagnostics);
      this.buildId = buildId;
      this.mainFile = build.main ?? null;
      this.warnings = warnings;
      if (build.success && build.page_count > 0) {
        this.status = 'success';
        this.pageCount = build.page_count;
        this.errors = [];
        this.message = 'Showing the last compile';
      } else {
        this.status = 'error';
        this.errors = errors.length > 0 ? errors : [{ message: 'The last compile failed' }];
        this.message = 'Showing the last compile';
      }
      return true;
    } catch {
      return false; // pruned or unreachable: start from a clean slate
    }
  }

  /** Forget the last result (a different project, say). */
  reset() {
    this.status = 'idle';
    this.progress = 0;
    this.message = '';
    this.jobId = null;
    this.buildId = null;
    this.mainFile = null;
    this.pageCount = 0;
    this.errors = [];
    this.warnings = [];
    this.candidates = [];
    this.lastCompileAt = null;
  }

  #onEvent(event, data) {
    if (event === 'progress') {
      this.progress = Math.round((data.progress ?? 0) * 100);
      this.message = data.message ?? this.message;
    } else if (event === 'compiled') {
      const { errors, warnings } = splitDiagnostics(data.diagnostics);
      this.buildId = data.build_id ?? null;
      this.mainFile = data.main_file ?? this.mainFile;
      this.warnings = warnings;
      this.lastCompileAt = new Date();
      if (data.success) {
        this.status = 'success';
        this.progress = 100;
        this.pageCount = data.page_count ?? 1;
        this.errors = [];
        this.message = 'Compiled';
      } else {
        this.status = 'error';
        this.progress = 0;
        // Located problems when the server found them, else the summary.
        this.errors = errors.length > 0 ? errors : [{ message: data.error_summary ?? 'Compilation failed' }];
        this.message = 'Compile failed';
      }
    } else if (event === 'error') {
      this.#fail(data.message);
    }
  }

  #fail(message) {
    this.status = 'error';
    this.progress = 0;
    this.message = message;
    if (this.errors.length === 0) this.errors = [{ message }];
  }
}
