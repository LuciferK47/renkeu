import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { APIClient, APIError } from '../src/lib/api/client.js';

describe('APIClient save contract', () => {
  let client;
  beforeEach(() => {
    client = new APIClient();
    vi.stubGlobal('fetch', vi.fn());
  });
  afterEach(() => vi.unstubAllGlobals());

  it('refuses to save without an ETag instead of sending a blind overwrite', async () => {
    await expect(client.saveFile('a.tex', 'x', null)).rejects.toMatchObject({
      status: 428
    });
    expect(fetch).not.toHaveBeenCalled();
  });

  it('sends If-Match with the ETag', async () => {
    fetch.mockResolvedValue(new Response('{"etag":"new","history_id":1}', { status: 200 }));
    await client.saveFile('a.tex', 'x', 'old');
    const [, init] = fetch.mock.calls[0];
    expect(init.headers['If-Match']).toBe('old');
  });

  it('surfaces the winning ETag on a 409', async () => {
    fetch.mockResolvedValue(
      new Response('{"error":"etag_conflict","current_etag":"theirs"}', { status: 409 })
    );
    const err = await client.saveFile('a.tex', 'x', 'old').catch((e) => e);
    expect(err).toBeInstanceOf(APIError);
    expect(err.status).toBe(409);
    expect(err.payload.current_etag).toBe('theirs');
  });

  it('refuses to apply hunks without the ETag they were proposed against', async () => {
    await expect(
      client.applySelectedHunks('a.tex', [], ['h1'], null)
    ).rejects.toMatchObject({ status: 428 });
    expect(fetch).not.toHaveBeenCalled();
  });

  it('sends the ETag and instruction at request level when applying', async () => {
    fetch.mockResolvedValue(new Response('{"success":true}', { status: 200 }));
    await client.applySelectedHunks('a.tex', [{ op: 'insert' }], ['h1'], 'etag1', 'fix it');
    const body = JSON.parse(fetch.mock.calls[0][1].body);
    expect(body.expected_etag).toBe('etag1');
    expect(body.instruction).toBe('fix it');
    expect(body.edits[0].expected_etag).toBeUndefined();
  });
});
