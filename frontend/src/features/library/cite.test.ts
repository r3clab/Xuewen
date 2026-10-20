import { beforeEach, describe, expect, it, vi } from 'vitest';
import { copyCitation } from './cite';
import { bibFormat } from './state.svelte';

describe('copyCitation', () => {
  beforeEach(() => {
    bibFormat.value = 'bibtex';
    vi.unstubAllGlobals();
  });

  it('fetches the entry in the current format and writes it to the clipboard', async () => {
    const writeText = vi.fn(async () => {});
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    let requested = '';
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string | URL) => {
        requested = String(url);
        return new Response('@article{x,\n}', { status: 200 });
      }),
    );

    bibFormat.value = 'biblatex';
    await copyCitation('aaaa1111');

    expect(requested).toBe('/api/papers/aaaa1111/export?format=biblatex');
    expect(writeText).toHaveBeenCalledWith('@article{x,\n}');
  });

  it('falls back to execCommand when the Clipboard API is unavailable (insecure http context)', async () => {
    // Plain HTTP on a non-localhost host: navigator.clipboard is undefined, so
    // the async Clipboard API path is not usable.
    vi.stubGlobal('navigator', {});
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('@article{x,\n}', { status: 200 })),
    );
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
