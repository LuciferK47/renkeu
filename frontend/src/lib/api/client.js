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

  // `paper` and `warmth` tone the page for a dark UI. The server compresses
  // lightness in OKLab and leaves hue alone, so a red warning box stays red --
  // unlike the `filter: invert()` every other LaTeX viewer resorts to.
  // paper === 1 means "send it exactly as it prints", which is what
  // hold-to-reveal asks for.
  pdfPageUrl(filePath, page, dpi, tone) {
    const params = new URLSearchParams({ file: filePath, n: String(page) });
    if (dpi) params.set('dpi', String(dpi));
    if (tone) {
      if (tone.paper != null) params.set('paper', String(tone.paper));
      if (tone.warmth != null) params.set('warmth', String(tone.warmth));
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

  // `expectedEtag` makes the write conditional on the file not having moved
  // while the user reviewed; a 409 carries the winning etag.
  applySelectedHunks(filePath, edits, selectedHunkIds, expectedEtag = null) {
    const payload = expectedEtag
      ? edits.map((e) => ({ ...e, expected_etag: expectedEtag }))
      : edits;
    return this.request('POST', '/agent/apply-selected', {
      body: { file_path: filePath, edits: payload, selected_hunk_ids: selectedHunkIds }
    });
  }

  // Region coordinates are pixels of the page image rendered at `dpi`.
  repairVisualCrop({ filePath, page, x, y, w, h, dpi, instruction }) {
    return this.request('POST', '/visual-fix', {
      body: {
        file_path: filePath,
        page,
        x,
        y,
        w,
        h,
        ...(dpi ? { dpi } : {}),
        ...(instruction ? { instruction } : {})
      }
    });
  }

  getConfig() {
    return this.request('GET', '/config');
  }
}

export const api = new APIClient();
