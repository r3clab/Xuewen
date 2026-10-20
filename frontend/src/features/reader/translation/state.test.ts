import { beforeEach, describe, expect, it, vi } from 'vitest';
import { requestTranslate, translateBox, closeTranslate, translateTrigger } from './state.svelte';
import { appSettings } from '$shared/api/settings.svelte';

beforeEach(() => {
  closeTranslate();
  vi.unstubAllGlobals();
});

describe('requestTranslate', () => {
  it('opens the box, shows loading, then fills the translation', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        new Response(
          JSON.stringify({ translation: '你好', provider: 'llm', source_lang: 'EN', target_lang: 'zh' }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      ),
    );
    await requestTranslate('hello', { x: 100, y: 200 });
    expect(translateBox.open).toBe(true);
    expect(translateBox.translation).toBe('你好');
    expect(translateBox.provider).toBe('llm');
    expect(translateBox.loading).toBe(false);
    expect(translateBox.error).toBeNull();
  });

  it('records an error when the request fails', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('nope', { status: 502 })));
    await requestTranslate('hello', { x: 0, y: 0 });
    expect(translateBox.open).toBe(true);
    expect(translateBox.error).not.toBeNull();
    expect(translateBox.loading).toBe(false);
  });

  it('does not let a superseded (out-of-order) response clobber a newer request', async () => {
    // deferred promises so we control exactly when each fetch resolves, and
    // resolve them out of order: the SECOND request ('b') finishes first,
    // then the first request ('a') resolves late and must be ignored.
    function deferred<T>() {
      let resolve!: (v: T) => void;
      const promise = new Promise<T>((r) => {
        resolve = r;
      });
      return { promise, resolve };
    }
    const first = deferred<Response>();
    const second = deferred<Response>();
    const calls: Array<{ promise: Promise<Response> }> = [first, second];
    let callIndex = 0;
    vi.stubGlobal(
      'fetch',
      vi.fn(() => calls[callIndex++].promise),
    );

    const pA = requestTranslate('a', { x: 0, y: 0 });
    const pB = requestTranslate('b', { x: 0, y: 0 });

    // 'b' (the newer request) resolves first...
    second.resolve(
      new Response(
        JSON.stringify({ translation: 'B translated', provider: 'llm', source_lang: 'EN', target_lang: 'zh' }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      ),
    );
    await pB;

    // ...then the stale 'a' request resolves late and must be ignored.
    first.resolve(
      new Response(
        JSON.stringify({ translation: 'A translated', provider: 'llm', source_lang: 'EN', target_lang: 'zh' }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      ),
    );
    await pA;

    expect(translateBox.source).toBe('b');
    expect(translateBox.translation).toBe('B translated');
    expect(translateBox.loading).toBe(false);
    expect(translateBox.error).toBeNull();
  });
});

describe('translateTrigger', () => {
  it('reads the trigger from server settings, defaulting to auto', () => {
    appSettings.translate = { enabled: true, providers: ['llm'], default_provider: 'llm', target_lang: 'zh', trigger: 'manual' };
    expect(translateTrigger()).toBe('manual');
    appSettings.translate = { enabled: true, providers: ['llm'], default_provider: 'llm', target_lang: 'zh' };
    expect(translateTrigger()).toBe('auto');
  });
});
