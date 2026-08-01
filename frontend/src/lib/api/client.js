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

const BASE_URL = import.meta.env.VITE_API_BASE || '/api';

export class APIClient {
  buildUrl(endpoint) {
    return `${BASE_URL}${endpoint}`;
  }

  async request(method, path, { body, headers = {}, params, signal } = {}) {
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
  saveFile(path, content, etag) {
    return this.request('POST', '/file', {
      body: { path, content },
      headers: etag ? { 'If-Match': etag } : {}
    });
  }

  listFiles() {
    return this.request('GET', '/files');
  }

  // --- search ------------------------------------------------------------

  // Server-side nucleo fuzzy match over the workspace file list.
  searchWorkspace(q) {
    return this.request('GET', '/search', { params: { q } });
  }

  // Client supplies candidates (command palette entries).
  searchCandidates(query, candidates) {
    return this.request('POST', '/search', { body: { query, candidates } });
  }

  // --- compile -----------------------------------------------------------

  // -> { job_id, status }
  compile(filePath, signal) {
    return this.request('POST', '/compile', { body: { file_path: filePath }, signal });
  }

  compileStreamUrl(jobId) {
    return this.buildUrl(`/compile/stream/${jobId}`);
  }

  pdfPageUrl(filePath, page, dpi) {
    const params = new URLSearchParams({ file: filePath, n: String(page) });
    if (dpi) params.set('dpi', String(dpi));
    return `${this.buildUrl('/pdf/page')}?${params.toString()}`;
  }

  // --- agent -------------------------------------------------------------

  applySelectedHunks(filePath, edits, selectedHunkIds) {
    return this.request('POST', '/agent/apply-selected', {
      body: { file_path: filePath, edits, selected_hunk_ids: selectedHunkIds }
    });
  }

  repairVisualCrop(filePath, page, x, y, w, h) {
    return this.request('POST', '/visual-fix', {
      body: { file_path: filePath, page, x, y, w, h }
    });
  }

  getConfig() {
    return this.request('GET', '/config');
  }
}

export const api = new APIClient();
