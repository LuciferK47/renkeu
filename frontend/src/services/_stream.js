// The one canonical Server-Sent-Events parser (masterplan §10.3). Reads a
// streaming fetch Response and yields { event, data } per SSE message. Buffers
// across network chunks so a message split mid-line is still parsed correctly.
export async function* streamSSE(response) {
  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  let event = 'message'
  while (true) {
    const { done, value } = await reader.read()
    if (done) break
    buffer += decoder.decode(value, { stream: true })
    const lines = buffer.split('\n')
    buffer = lines.pop() || ''
    for (const line of lines) {
      if (line.startsWith('event: ')) {
        event = line.slice(7).trim()
      } else if (line.startsWith('data: ')) {
        const payload = line.slice(6)
        if (!payload.trim()) continue
        yield { event, data: JSON.parse(payload) }
        event = 'message'
      }
    }
  }
}
