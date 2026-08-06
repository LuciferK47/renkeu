// The SSE parser is the single contract point between every backend stream
// and the UI — masterplan §12 M2 audit gate requires it to be unit-tested.
import { describe, it, expect } from 'vitest';
import { streamSSE } from '../src/services/_stream.js';

/** Build a Response-like object that emits the given string chunks. */
function responseOf(chunks) {
  const encoder = new TextEncoder();
  let i = 0;
  return {
    body: {
      getReader: () => ({
        read: async () =>
          i < chunks.length
            ? { done: false, value: encoder.encode(chunks[i++]) }
            : { done: true, value: undefined },
        releaseLock: () => {}
      })
    }
  };
}

async function collect(chunks) {
  const out = [];
  for await (const message of streamSSE(responseOf(chunks))) out.push(message);
  return out;
}

describe('streamSSE', () => {
  it('parses event name and JSON payload', async () => {
    const events = await collect(['event: progress\ndata: {"message":"hi"}\n\n']);
    expect(events).toEqual([{ event: 'progress', data: { message: 'hi' } }]);
  });

  it('buffers messages split across network chunks', async () => {
    const events = await collect(['event: prop', 'osal\ndata: {"edit_c', 'ount":2}\n\n']);
    expect(events).toEqual([{ event: 'proposal', data: { edit_count: 2 } }]);
  });

  it('parses a full agent-loop taxonomy sequence in order', async () => {
    const events = await collect([
      'event: thinking\ndata: {"iteration":1}\n\n',
      'event: proposal\ndata: {"edit_count":1}\n\n',
      'event: applying\ndata: {}\n\n',
      'event: compiled\ndata: {"success":true}\n\n',
      'event: done\ndata: {"success":true}\n\n'
    ]);
    expect(events.map((e) => e.event)).toEqual([
      'thinking',
      'proposal',
      'applying',
      'compiled',
      'done'
    ]);
    expect(events[3].data.success).toBe(true);
  });

  it('resets the event name between messages', async () => {
    const events = await collect(['event: error\ndata: {"message":"x"}\n\ndata: {"plain":1}\n\n']);
    expect(events[0].event).toBe('error');
    expect(events[1].event).toBe('message');
  });

  it('surfaces a non-JSON payload as a string instead of throwing', async () => {
    // Regression: the old parser called JSON.parse unconditionally, so one
    // bare-string frame killed every stream.
    const events = await collect(['event: progress\ndata: Thinking...\n\n']);
    expect(events).toEqual([{ event: 'progress', data: 'Thinking...' }]);
  });

  it('handles CRLF line endings and comment keep-alives', async () => {
    const events = await collect([': keep-alive\r\n\r\nevent: done\r\ndata: {"success":true}\r\n\r\n']);
    expect(events).toEqual([{ event: 'done', data: { success: true } }]);
  });

  it('joins multi-line data frames', async () => {
    const events = await collect(['event: done\ndata: {"a":1,\ndata: "b":2}\n\n']);
    expect(events[0].data).toEqual({ a: 1, b: 2 });
  });

  it('emits a trailing message even without a final blank line', async () => {
    const events = await collect(['event: done\ndata: {"success":false}\n']);
    expect(events).toEqual([{ event: 'done', data: { success: false } }]);
  });

  it('ignores empty data frames', async () => {
    const events = await collect(['event: progress\ndata: \n\nevent: done\ndata: {}\n\n']);
    expect(events).toEqual([{ event: 'done', data: {} }]);
  });

  it('throws a clear error when the response has no body', async () => {
    await expect(async () => {
      for await (const _ of streamSSE({})) {
        /* unreachable */
      }
    }).rejects.toThrow(/no body/);
  });
});
