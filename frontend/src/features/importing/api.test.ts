import { afterEach, describe, expect, it, vi } from 'vitest';
import { importPaper } from './api';

function stubFetch(handler: (url: string, init?: RequestInit) => Response) {
  const spy = vi.fn(async (url: string | URL, init?: RequestInit) => handler(String(url), init));
  vi.stubGlobal('fetch', spy);
  return spy;
}

afterEach(() => vi.unstubAllGlobals());

describe('request bodies', () => {
  it('sends FormData without a content-type', async () => {
    const spy = stubFetch(() => new Response('{"outcome":"duplicate"}', { status: 200 }));
    await importPaper(new File(['%PDF'], 'x.pdf'));
    const [, uploadInit] = spy.mock.calls[0];
    expect(uploadInit?.body).toBeInstanceOf(FormData);
    // The browser must pick the multipart content-type (with its boundary).
    expect(uploadInit?.headers).toBeUndefined();
  });
});
