import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiError } from '$shared/api/http';
import { renameTag } from './api';

function stubFetch(handler: (url: string, init?: RequestInit) => Response) {
  const spy = vi.fn(async (url: string | URL, init?: RequestInit) => handler(String(url), init));
  vi.stubGlobal('fetch', spy);
  return spy;
}

afterEach(() => vi.unstubAllGlobals());

describe('request error extraction', () => {
  it('surfaces the server {error} body from any endpoint', async () => {
    stubFetch(
      () =>
        new Response(JSON.stringify({ error: 'a tag with that name already exists' }), {
          status: 409,
        }),
    );
    await expect(renameTag('t1', 'nlp')).rejects.toThrow('a tag with that name already exists');
  });

  it('falls back to "<label>: <status>" and carries the status without an {error} body', async () => {
    stubFetch(() => new Response('boom', { status: 500 }));
    const err: unknown = await renameTag('t1', 'nlp').catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect((err as ApiError).message).toBe('rename tag failed: 500');
    expect((err as ApiError).status).toBe(500);
  });
});

describe('request bodies', () => {
  it('sends json with the JSON content-type', async () => {
    const spy = stubFetch(() => new Response('{"outcome":"duplicate"}', { status: 200 }));
    await renameTag('t1', 'nlp');
    const [, jsonInit] = spy.mock.calls[0];
    expect(jsonInit?.method).toBe('PATCH');
    expect(jsonInit?.headers).toEqual({ 'content-type': 'application/json' });
    expect(jsonInit?.body).toBe(JSON.stringify({ name: 'nlp' }));
  });
});
