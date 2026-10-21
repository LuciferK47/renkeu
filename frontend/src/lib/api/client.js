// Single API client. Native fetch (no axios — locked decision, masterplan §10.1).
// Every method here maps to a route that exists in crates/latex_ai_server.

export class APIError extends Error {
  constructor(message, status, payload) {
    super(message);
    this.name = 'APIError';
    this.status = status;
    this.payload = payload;
  }
}

// In the desktop shell the page is served from `tauri://localhost`, so a
// relative '/api' would resolve against that origin instead of the embedded
// server. The shell injects its real loopback base before any page script
// runs; in the browser we fall back to the dev-proxy path.
const BASE_URL = globalThis.__LATEX_AI_API__ || import.meta.env.VITE_API_BASE || '/api';

export class APIClient {
  buildUrl(endpoint) {
    return `${BASE_URL}${endpoint}`;
  }

  async request(method, path, { body, headers = {}, params, signal } = {}) {
    // `buildUrl` may return an absolute URL (desktop) or a path (browser);
    // the second argument is ignored for the former.
    const url = new URL(this.buildUrl(path), window.location.origin);
    if (params) {
      for (const [k, v] of Object.entries(params)) {
        if (v !== undefined && v !== null) url.searchParams.set(k, v);
      }
    }
    const opts = { method, headers: { ...headers }, signal };
    if (body !== undefined) {
      opts.headers['Content-Type'] = 'application/json';
      opts.body = JSON.stringify(body);
    }

    let res;
    try {
      res = await fetch(url, opts);
    } catch (err) {
      throw new APIError(err.message, 0, null);
    }

    const text = await res.text();
    let data = null;
    if (text) {
      try {
        data = JSON.parse(text);
      } catch {
        data = text;
      }
    }
    if (!res.ok) {
      // Backend error envelope: { "error": "<variant>", ...fields }
      const msg = (data && (data.message || data.error)) || res.statusText || 'request failed';
      throw new APIError(msg, res.status, data);
    }
    return data;
  }

  // --- files -------------------------------------------------------------

  // -> { content, etag, modified_at }
  getFile(path) {
    return this.request('GET', '/file', { params: { path } });
  }

  // If-Match drives optimistic concurrency; a 409 carries { current_etag }.
  // The server refuses a save with no etag (428), so fail here with a clear
  // message instead of sending a request that is certain to be rejected.
  // `autosave` marks a timer-driven save so the server folds a burst of them
  // into one history entry.
  saveFile(path, content, etag, { autosave = false } = {}) {
    if (!etag) {
      return Promise.reject(
        new APIError('Cannot save before the file has been loaded (no ETag).', 428, null)
      );
    }
    return this.request('POST', '/file', {
      body: { path, content, ...(autosave ? { autosave: true } : {}) },
      headers: { 'If-Match': etag }
    });
  }

  // --- file management ---------------------------------------------------

  // New text file (parent folders are created). -> { path, etag }
  createFile(path, content = '') {
    return this.request('POST', '/files/create', { body: { path, content } });
  }

  // Move a file or folder; the server refuses to replace anything (409).
  renameFile(from, to) {
    return this.request('POST', '/files/rename', { body: { from, to } });
  }

  // Needs the file's current ETag, like a save (428 without, 409 if stale).
  deleteFile(path, etag) {
    return this.request('DELETE', '/file', { params: { path }, headers: { 'If-Match': etag } });
  }

  // ETag of any project file, binary included, without downloading it.
  async fileEtag(path) {
    const url = new URL(this.rawUrl(path), window.location.origin);
    let res;
    try {
      res = await fetch(url, { method: 'HEAD' });
    } catch (err) {
      throw new APIError(err.message, 0, null);
    }
    if (!res.ok) throw new APIError(res.statusText || 'request failed', res.status, null);
    return res.headers.get('etag')?.replace(/"/g, '') ?? null;
  }

  // The request body is the file itself; `file` is a File or Blob.
  async uploadFile(path, file) {
    const url = new URL(this.buildUrl('/upload'), window.location.origin);
    url.searchParams.set('path', path);
    let res;
    try {
      res = await fetch(url, { method: 'POST', body: file });
    } catch (err) {
      throw new APIError(err.message, 0, null);
    }
    const data = await res.json().catch(() => null);
    if (!res.ok) throw new APIError((data && (data.message || data.error)) || res.statusText, res.status, data);
    return data;
  }

  // Bytes of a project file with their content type (for the image preview).
  rawUrl(path) {
    return `${this.buildUrl('/raw')}?path=${encodeURIComponent(path)}`;
  }

  // -> { main: string|null }; the root document the user picked.
  getProject() {
    return this.request('GET', '/project');
  }

  // Starter templates: the gallery, a new project from one (empty projects
  // only, 409 otherwise), and the front-matter block for a set of values.
  getTemplates() {
    return this.request('GET', '/templates');
  }

  templateThumbnailUrl(id) {
    return this.buildUrl(`/templates/${encodeURIComponent(id)}/thumbnail`);
  }

  startFromTemplate(template, fields) {
    return this.request('POST', '/project/template', { body: { template, fields } });
  }

  renderFrontmatter(template, fields) {
    return this.request('POST', '/templates/frontmatter', { body: { template, fields } });
  }

  // Layout facts for a build: pages, a page budget, boxes, short last lines, fonts.
  getLayout(build, pages) {
    return this.request('GET', '/layout', { params: { build, pages } });
  }

  // BibTeX for a DOI, fetched by the server from doi.org (the DOI is all that is sent).
  lookupDoi(doi) {
    return this.request('GET', '/doi', { params: { doi } });
  }

  // Find and replace across the project's text files. Replace writes through
  // the store, so every change is in the history.
  searchProject(body) {
    return this.request('POST', '/project/search', { body });
  }

  replaceProject(body) {
    return this.request('POST', '/project/replace', { body });
  }

  setMain(main) {
    return this.request('PUT', '/project', { body: { main } });
  }

  // The whole project as a nested tree: [{ name, path, kind: 'dir'|'file',
  // editable, children? }]. Secrets and dot-directories are never listed.
  getTree() {
    return this.request('GET', '/tree');
  }

  // -> [{ id, file_path, command, diff, edit_type, created_at }]
  getHistory(params) {
    return this.request('GET', '/history', { params });
  }

  // One entry with the text it recorded -> { id, file_path, edit_type,
  // created_at, text_after, text_before }. Used to restore a version.
  getHistoryEntry(id) {
    return this.request('GET', `/history/${encodeURIComponent(id)}`);
  }

  // --- search ------------------------------------------------------------

  // Server-side nucleo fuzzy match over the workspace file list.
  searchWorkspace(q) {
    return this.request('GET', '/search', { params: { q } });
  }

  // --- compile -----------------------------------------------------------

  // -> { job_id, status }
  // `main` names the root document when the project has several candidates
  // (otherwise the server picks it: \documentclass, `% !TEX root`, ...).
  compile(filePath, signal, main = null) {
    return this.request('POST', '/compile', {
      body: { file_path: filePath, ...(main ? { main } : {}) },
      signal
    });
  }

  compileStreamUrl(jobId) {
    return this.buildUrl(`/compile/stream/${jobId}`);
  }

  // `paper` and `warmth` tone the page for a dark UI. The server compresses
  // lightness in OKLab and leaves hue alone, so a red warning box stays red --
  // unlike the `filter: invert()` every other LaTeX viewer resorts to.
  // paper === 1 means "send it exactly as it prints", which is what
  // hold-to-reveal asks for.
  //
  // Pass `buildId` (from the `compiled` event) so the URL names exactly one
  // compile: it never goes stale and the server lets the browser cache it.
  // Without it the server serves "the newest build of this file".
  pdfPageUrl(filePath, page, dpi, tone, buildId) {
    const params = new URLSearchParams({ n: String(page) });
    if (buildId) params.set('build', buildId);
    else params.set('file', filePath);
    if (dpi) params.set('dpi', String(dpi));
    if (tone) {
      if (tone.paper !== undefined && tone.paper !== null) params.set('paper', String(tone.paper));
      if (tone.warmth !== undefined && tone.warmth !== null) params.set('warmth', String(tone.warmth));
    }
    return `${this.buildUrl('/pdf/page')}?${params.toString()}`;
  }

  // --- agent -------------------------------------------------------------

  // Per-hunk diffs computed server-side, so what is shown matches what a
  // subsequent apply will do. -> { etag, hunks: [{hunk_id, diff, applies, error}] }
  previewHunks(filePath, edits) {
    return this.request('POST', '/agent/preview', {
      body: { file_path: filePath, edits }
    });
  }

  // `expectedEtag` is required: it makes the write conditional on the file not
  // having moved while the user reviewed; a 409 carries the winning etag.
  // `instruction` is stored in the history row. `proposalId` (from the
  // proposal event) lets the server check files other than `filePath` and
  // lets `rebase` move the hunks onto a file that changed during review.
  applySelectedHunks(
    filePath,
    edits,
    selectedHunkIds,
    expectedEtag,
    instruction = null,
    { proposalId = null, rebase = false } = {}
  ) {
    if (!expectedEtag) {
      return Promise.reject(
        new APIError('Cannot apply edits without the ETag of the file they were proposed for.', 428, null)
      );
    }
    return this.request('POST', '/agent/apply-selected', {
      body: {
        file_path: filePath,
        edits,
        selected_hunk_ids: selectedHunkIds,
        expected_etag: expectedEtag,
        ...(instruction ? { instruction } : {}),
        ...(proposalId ? { proposal_id: proposalId } : {}),
        ...(rebase ? { rebase: true } : {})
      }
    });
  }

  // The region is in PDF points from the page's top-left corner, so it means
  // the same thing at any zoom.
  repairVisualCrop({ filePath, buildId, page, x, y, w, h, instruction }) {
    return this.request('POST', '/visual-fix', {
      body: {
        file_path: filePath,
        ...(buildId ? { build: buildId } : {}),
        page,
        x,
        y,
        w,
        h,
        ...(instruction ? { instruction } : {})
      }
    });
  }

  // --- SyncTeX -----------------------------------------------------------

  // Source line -> where it landed in the PDF.
  // -> { build, stale, hits: [{ page, x, y, width, height, line }] } (points)
  synctexForward({ file, line, build }) {
    return this.request('GET', '/synctex/forward', { params: { file, line, build } });
  }

  // A point on a page (PDF points) -> the source line behind it.
  // -> { file, line, column, exact, stale }
  synctexBackward({ build, page, x, y }) {
    return this.request('GET', '/synctex/backward', { params: { build, page, x, y } });
  }

  getConfig() {
    return this.request('GET', '/config');
  }

  // --- builds ------------------------------------------------------------

  // Stop a running compile; the worker process is killed server-side.
  cancelCompile(jobId) {
    return this.request('POST', `/compile/${encodeURIComponent(jobId)}/cancel`);
  }

  // What a compile found: success, main file, diagnostics with file and line.
  getBuild(buildId) {
    return this.request('GET', `/builds/${encodeURIComponent(buildId)}`);
  }

  // The TeX log of a build, as text (the tail, if it is very long).
  getBuildLog(buildId) {
    return this.request('GET', `/builds/${encodeURIComponent(buildId)}/log`);
  }

  // URL that downloads the finished PDF ("Export PDF").
  exportPdfUrl(buildId) {
    return this.buildUrl(`/builds/${encodeURIComponent(buildId)}/pdf`);
  }

  // Partial update of the server-side settings (model, vision_model,
  // timeout_secs). Unknown fields are rejected with 422, and fields pinned by
  // an environment variable with 409.
  saveConfig(patch) {
    return this.request('PATCH', '/config', { body: patch });
  }

  // The key is write-only: no response ever contains it.
  setApiKey(key) {
    return this.request('PUT', '/config/api-key', { body: { key } });
  }

  clearApiKey() {
    return this.request('DELETE', '/config/api-key');
  }
}

export const api = new APIClient();
