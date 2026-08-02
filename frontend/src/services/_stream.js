// The one canonical Server-Sent-Events parser (masterplan §10.3). Reads a
// streaming fetch Response and yields { event, data } per SSE message.
//
// Robustness notes (each earned by a real breakage):
// - buffers across network chunks so a message split mid-line still parses;
// - handles CRLF as well as LF line endings;
// - a payload that is not valid JSON is surfaced as a raw string instead of
//   throwing, so one malformed frame cannot kill the whole stream;
// - honours multi-line `data:` frames per the SSE spec.
export async function* streamSSE(response) {
  if (!response.body) throw new Error('response has no body to stream');

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  let event = 'message';
  let data = [];

  const flush = () => {
    if (data.length === 0) return null;
    const raw = data.join('\n');
    data = [];
    const name = event;
    event = 'message';
    if (!raw.trim()) return null;
    let parsed;
    try {
      parsed = JSON.parse(raw);
    } catch {
      parsed = raw;
    }
    return { event: name, data: parsed };
  };

  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });

      const lines = buffer.split('\n');
      buffer = lines.pop() ?? '';

      for (const rawLine of lines) {
        const line = rawLine.endsWith('\r') ? rawLine.slice(0, -1) : rawLine;

        if (line === '') {
          const message = flush();
          if (message) yield message;
        } else if (line.startsWith(':')) {
          // comment / keep-alive
        } else if (line.startsWith('event:')) {
          event = line.slice(6).trim();
        } else if (line.startsWith('data:')) {
          data.push(line.slice(5).replace(/^ /, ''));
        }
      }
    }

    // Stream ended without a trailing blank line.
    const message = flush();
    if (message) yield message;
  } finally {
    reader.releaseLock?.();
  }
}

/** Events after which the server is finished and closing is expected. */
const TERMINAL = new Set(['done', 'error', 'awaiting_approval']);

/**
 * Consume an SSE response, reporting a stream that stops early.
 *
 * A dropped connection does not always throw: the reader can simply report
 * done, in which case `for await` ends quietly and the caller cannot tell a
 * finished run from a severed one. That is how a mid-run agent failure used to
 * present -- the transcript stopped, the status bar went back to Ready, and
 * nothing said the work had been abandoned.
 *
 * Returns true when the stream ended on a terminal event.
 */
export async function consumeSSE(response, onEvent) {
  let sawTerminal = false;
  for await (const message of streamSSE(response)) {
    if (TERMINAL.has(message.event)) sawTerminal = true;
    onEvent(message);
  }
  return sawTerminal;
}
