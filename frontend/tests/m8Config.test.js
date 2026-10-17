import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import { APIClient } from '../src/lib/api/client.js';
import ChatPanel from '../src/components/ChatPanel.svelte';

describe('config client', () => {
  let client;
  beforeEach(() => {
    client = new APIClient();
    vi.stubGlobal('fetch', vi.fn());
  });
  afterEach(() => vi.unstubAllGlobals());

  const ok = () => new Response('{"ai_enabled":true}', { status: 200 });

  it('saves settings with PATCH, not the old 405 POST', async () => {
    fetch.mockImplementation(async () => ok());
    await client.saveConfig({ model: 'm' });
    const [url, init] = fetch.mock.calls[0];
    expect(String(url)).toMatch(/\/config$/);
    expect(init.method).toBe('PATCH');
    expect(JSON.parse(init.body)).toEqual({ model: 'm' });
  });

  it('sets and clears the API key through its own write-only route', async () => {
    fetch.mockImplementation(async () => ok());
    await client.setApiKey('gsk_abc');
    let [url, init] = fetch.mock.calls[0];
    expect(String(url)).toMatch(/\/config\/api-key$/);
    expect(init.method).toBe('PUT');
    expect(JSON.parse(init.body)).toEqual({ key: 'gsk_abc' });

    await client.clearApiKey();
    [url, init] = fetch.mock.calls[1];
    expect(init.method).toBe('DELETE');
    expect(init.body).toBeUndefined();
  });

  it('surfaces an env-pinned field as a 409 with the server message', async () => {
    fetch.mockResolvedValue(
      new Response('{"error":"conflict","message":"model is set by an environment variable"}', {
        status: 409
      })
    );
    await expect(client.saveConfig({ model: 'x' })).rejects.toMatchObject({ status: 409 });
  });
});

describe('ChatPanel without an API key', () => {
  it('explains how to enable AI and blocks sending', () => {
    render(ChatPanel, { props: { aiEnabled: false } });
    expect(screen.getByRole('status').textContent).toMatch(/API key/);
    expect(screen.getByRole('textbox').disabled).toBe(true);
  });

  it('is a normal chat when AI is on', () => {
    render(ChatPanel, { props: { aiEnabled: true } });
    expect(screen.queryByRole('status')).toBeNull();
    expect(screen.getByRole('textbox').disabled).toBe(false);
  });
});
