// Single API client. Native fetch (no axios — locked decision, masterplan §10.1).
// Methods mirror the contract the services import; endpoints map to the Rust
// backend (some land in M2/M3).

export class APIError extends Error {
  constructor(message, status, payload) {
    super(message)
    this.name = 'APIError'
    this.status = status
    this.payload = payload
  }
}

const BASE_URL = import.meta.env.VITE_API_BASE || '/api'

export class APIClient {
  buildUrl(endpoint) {
    return `${BASE_URL}${endpoint}`
  }

  async request(method, path, { body, headers = {}, params, signal } = {}) {
    const url = new URL(this.buildUrl(path), window.location.origin)
    if (params) {
      for (const [k, v] of Object.entries(params)) {
        if (v !== undefined && v !== null) url.searchParams.set(k, v)
      }
    }
    const opts = { method, headers: { ...headers }, signal }
    if (body !== undefined) {
      opts.headers['Content-Type'] = 'application/json'
      opts.body = JSON.stringify(body)
    }

    let res
    try {
      res = await fetch(url, opts)
    } catch (err) {
      throw new APIError(err.message, 0, null)
    }

    const text = await res.text()
    let data = null
    if (text) {
      try {
        data = JSON.parse(text)
      } catch {
        data = text
      }
    }
    if (!res.ok) {
      const msg = (data && data.error) || res.statusText || 'request failed'
      throw new APIError(msg, res.status, data)
    }
    return data
  }

  getFile(path) {
    return this.request('GET', '/file', { params: { path } })
  }

  saveFile(path, content, etag) {
    return this.request('POST', '/file', {
      body: { path, content },
      headers: etag ? { 'If-Match': etag } : {},
    })
  }

  compile(filePath, engine = 'pdflatex', signal) {
    return this.request('POST', '/compile', { body: { file_path: filePath, engine }, signal })
  }

  getCompileStatus(jobId, signal) {
    return this.request('GET', `/compile/status/${jobId}`, { signal })
  }

  checkCompileCache(filePath, outputDir = null, signal) {
    return this.request('POST', '/compile/check-cache', {
      body: { file_path: filePath, output_dir: outputDir },
      signal,
    })
  }

  syncTex(path, page, h, v) {
    return this.request('POST', '/synctex', { body: { file_path: path, page, h, v } })
  }

  getPdfUrl(jobId) {
    return `${BASE_URL}/compile/stream/${jobId}`
  }

  repairVisualCrop(filePath, page, x, y, w, h) {
    return this.request('POST', '/visual-fix', {
      body: { file_path: filePath, page, x, y, w, h }
    })
  }
}

export const api = new APIClient()
