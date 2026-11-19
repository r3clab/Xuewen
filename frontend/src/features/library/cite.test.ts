import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { copyCitation } from './cite';
import { bibFormat } from './state.svelte';

class FakeClipboardItem {
  constructor(readonly items: Record<string, Promise<Blob>>) {}
}

function stubExport(body = '@article{x,\n}', status = 200) {
  const fetch = vi.fn(async (_url: string | URL) => new Response(body, { status }));
  vi.stubGlobal('fetch', fetch);
  return fetch;
}

beforeEach(() => {
  bibFormat.value = 'bibtex';
});
afterEach(() => vi.unstubAllGlobals());

describe('copyCitation', () => {
  it('starts the clipboard write before the entry arrives and hands it the pending text', async () => {
    let written: FakeClipboardItem | undefined;
    const write = vi.fn(async ([item]: FakeClipboardItem[]) => {
      written = item;
    });
    vi.stubGlobal('ClipboardItem', FakeClipboardItem);
    vi.stubGlobal('navigator', { clipboard: { write } });
    const fetch = stubExport();

    bibFormat.value = 'biblatex';
    await copyCitation('aaaa1111');

    expect(fetch).toHaveBeenCalledWith('/api/papers/aaaa1111/export?format=biblatex', expect.anything());
    expect(write).toHaveBeenCalledOnce();
    expect(await (await written!.items['text/plain']).text()).toBe('@article{x,\n}');
  });

  it('reports a failed fetch rather than the write it starved', async () => {
    vi.stubGlobal('ClipboardItem', FakeClipboardItem);
    vi.stubGlobal('navigator', {
      clipboard: {
        write: async ([item]: FakeClipboardItem[]) => {
          await item.items['text/plain'];
        },
      },
    });
    stubExport(JSON.stringify({ error: 'no such paper' }), 404);
    await expect(copyCitation('gone')).rejects.toThrow('no such paper');
  });

  it('falls back to execCommand when the Clipboard API is unavailable (insecure http context)', async () => {
    vi.stubGlobal('navigator', {});
    stubExport();
    const exec = vi.fn(() => true);
    const original = document.execCommand;
    document.execCommand = exec as unknown as typeof document.execCommand;
    try {
      await expect(copyCitation('aaaa1111')).resolves.toBeUndefined();
      expect(exec).toHaveBeenCalledWith('copy');
    } finally {
      document.execCommand = original;
    }
  });
});
