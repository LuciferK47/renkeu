// Simple diff parsing worker to offload heavy processing from the UI thread
// Receives a unified diff string and returns an array of hunks

self.onmessage = (e) => {
  const { diffText } = e.data || {}
  if (typeof diffText !== 'string') {
    self.postMessage({ error: 'Invalid diffText' })
    return
  }
  try {
    const lines = diffText.split('\n')
    const hunks = []
    let current = null
    for (const line of lines) {
      if (line.startsWith('@@')) {
        if (current) hunks.push(current)
        current = { header: line, lines: [] }
        continue
      }
      if (!current) {
        current = { header: 'preamble', lines: [] }
      }
      let type = 'normal'
      if (line.startsWith('+')) type = 'add'
      else if (line.startsWith('-')) type = 'remove'
      else if (line.startsWith('@@')) type = 'meta'
      current.lines.push({ raw: line, type, text: line.replace(/^[-+]/, '') })
    }
    if (current) hunks.push(current)
    self.postMessage({ hunks })
  } catch (err) {
    self.postMessage({ error: err?.message || 'diff worker failed' })
  }
}
