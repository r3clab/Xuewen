import { render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ImportModal from './ImportModal.svelte';
import {
  closeImport,
  enqueueFiles,
  enqueueUrl,
  importState,
  openImport,
} from './queue.svelte';
import { appSettings } from '$shared/api/settings.svelte';

function pdf(name: string): File {
  return new File([new Uint8Array([0x25, 0x50, 0x44, 0x46])], name, {
    type: 'application/pdf',
  });
}

// A fetch stub: POST /api/papers -> per-file JSON body; GET list/stats -> empty.
// The HTTP status is 400 when the body carries an `error`, else 200 (so a
// paper's own `status: 'resolved'` string never gets mistaken for an HTTP code).
function stubFetch(outcome: (name: string) => object) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string | URL, init?: RequestInit) => {
      const u = String(url);
      const json = (o: unknown, status = 200) =>
        new Response(JSON.stringify(o), {
          status,
          headers: { 'content-type': 'application/json' },
        });
      if (u === '/api/papers' && init?.method === 'POST') {
        const f = (init.body as FormData).get('file') as File;
        const payload = outcome(f.name) as Record<string, unknown>;
        return json(payload, typeof payload.error === 'string' ? 400 : 200);
      }
      if (u.startsWith('/api/papers')) return json([]);
      return json({ total: 0, resolved: 0, needs_review: 0 });
    }),
  );
}

describe('enqueueFiles', () => {
  beforeEach(() => {
    openImport(); // resets importState
    vi.restoreAllMocks();
  });

  it('imports files sequentially and records each outcome', async () => {
    const seen: string[] = [];
    stubFetch((name) => {
      seen.push(name);
      return name === 'a.pdf'
        ? { outcome: 'ingested', id: '1', title: 'A', status: 'resolved' }
        : { outcome: 'duplicate' };
    });

    await enqueueFiles([pdf('a.pdf'), pdf('b.pdf')]);

    expect(seen).toEqual(['a.pdf', 'b.pdf']); // one at a time, in order
    expect(importState.items.map((i) => i.status)).toEqual(['ingested', 'duplicate']);
    expect(importState.items[0].message).toBe('A');
  });

  it('marks a rejected upload as failed with the server message', async () => {
    stubFetch(() => ({ error: 'not a PDF' }));

    await enqueueFiles([pdf('bad.pdf')]);

    expect(importState.items[0].status).toBe('failed');
    expect(importState.items[0].message).toBe('not a PDF');
  });

  it('drops a stale upload result from a superseded session', async () => {
    // Control exactly when slow.pdf's POST resolves.
    let releaseSlow!: () => void;
    const slowGate = new Promise<void>((r) => (releaseSlow = r));
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string | URL, init?: RequestInit) => {
        const u = String(url);
        const json = (o: unknown, status = 200) =>
          new Response(JSON.stringify(o), {
            status,
            headers: { 'content-type': 'application/json' },
          });
        if (u === '/api/papers' && init?.method === 'POST') {
          const f = (init.body as FormData).get('file') as File;
          if (f.name === 'slow.pdf') {
            await slowGate;
            return json({ outcome: 'ingested', id: 's', title: 'STALE', status: 'resolved' });
          }
          return json({ outcome: 'duplicate' });
        }
        if (u.startsWith('/api/papers')) return json([]);
        return json({ total: 0, resolved: 0, needs_review: 0 });
      }),
    );

    // Session A: start slow.pdf (stays in flight), then close.
    openImport();
    const batchA = enqueueFiles([pdf('slow.pdf')]);
    closeImport();

    // Session B: reset, enqueue a fresh file (shares the still-running drain loop).
    openImport();
    const batchB = enqueueFiles([pdf('fresh.pdf')]);

    // Let the stale slow.pdf resolve; the drain loop must drop it and process fresh.pdf.
    releaseSlow();
    await batchB;
    await batchA;

    expect(importState.items.length).toBe(1);
    expect(importState.items[0].status).toBe('duplicate');
    expect(importState.items[0].message).toBeUndefined(); // no 'STALE' leak
  });

  it('records same_work and in_trash outcomes', async () => {
    stubFetch((name) =>
      name === 'dup.pdf' ? { outcome: 'same_work', id: 'x1' } : { outcome: 'in_trash', id: 'x2' },
    );

    await enqueueFiles([pdf('dup.pdf'), pdf('trashed.pdf')]);

    expect(importState.items.map((i) => i.status)).toEqual(['same-work', 'in-trash']);
    expect(importState.items[1].message).toBe('x2');
  });
});

describe('enqueueUrl', () => {
  beforeEach(() => {
    openImport();
    vi.restoreAllMocks();
  });

  it('imports a URL and records ingested', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string | URL, init?: RequestInit) => {
        const u = String(url);
        const json = (o: unknown, status = 200) =>
          new Response(JSON.stringify(o), { status, headers: { 'content-type': 'application/json' } });
        if (u === '/api/import' && init?.method === 'POST') {
          return json({ outcome: 'ingested', id: '1', title: 'Fetched Paper', status: 'resolved' });
        }
        if (u.startsWith('/api/papers')) return json([]);
        return json({ total: 0, resolved: 0, needs_review: 0 });
      }),
    );

    await enqueueUrl('https://arxiv.org/abs/1706.03762');

    expect(importState.items).toHaveLength(1);
    expect(importState.items[0].status).toBe('ingested');
    expect(importState.items[0].message).toBe('Fetched Paper');
  });

  it('marks an unfetched result distinctly', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string | URL, init?: RequestInit) => {
        const u = String(url);
        const json = (o: unknown, status = 200) =>
          new Response(JSON.stringify(o), { status, headers: { 'content-type': 'application/json' } });
        if (u === '/api/import' && init?.method === 'POST') {
          return json({ outcome: 'unfetched', title: 'Paywalled Paper', doi: '10.1145/x' });
        }
        if (u.startsWith('/api/papers')) return json([]);
        return json({ total: 0, resolved: 0, needs_review: 0 });
      }),
    );

    await enqueueUrl('10.1145/x');

    expect(importState.items[0].status).toBe('unfetched');
    expect(importState.items[0].message).toBe('Paywalled Paper');
  });
});

describe('proxy cookie section', () => {
  // Settings GETs serve from `cookieSet`; the PUT/DELETE handlers mutate or
  // reject so the modal's inline error path and store refresh are observable.
  function stubSettingsFetch(opts: { failPut?: boolean; noProxy?: boolean } = {}) {
    let cookieSet = appSettings.proxyCookieSet;
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string | URL, init?: RequestInit) => {
        const u = String(url);
        const json = (o: unknown, status = 200) =>
          new Response(JSON.stringify(o), {
            status,
            headers: { 'content-type': 'application/json' },
          });
        if (u === '/api/settings/proxy-cookie' && init?.method === 'PUT') {
          if (opts.failPut) return json({ error: 'bad cookie' }, 400);
          cookieSet = true;
          return new Response(null, { status: 204 });
        }
        if (u === '/api/settings/proxy-cookie' && init?.method === 'DELETE') {
          cookieSet = false;
          return new Response(null, { status: 204 });
        }
        if (u === '/api/settings') {
          return json({
            proxy: opts.noProxy ? null : { host: 'proxy.example.edu' },
            proxy_cookie_set: cookieSet,
            proxy_cookie_updated_at: cookieSet ? '2026-08-14' : null,
            fold_abstract: true,
          });
        }
        return json([]);
      }),
    );
  }

  beforeEach(() => {
    openImport();
    appSettings.proxyHost = 'proxy.example.edu';
    appSettings.proxyCookieSet = false;
    appSettings.proxyCookieUpdatedAt = null;
    vi.unstubAllGlobals();
  });

  it('a rejected save shows the error inline and keeps the typed cookie', async () => {
    stubSettingsFetch({ failPut: true });
    render(ImportModal);
    await userEvent.click(screen.getByText(/institutional access/i));
    const input = screen.getByPlaceholderText(/ezproxy/);
    await userEvent.type(input, 'ezproxy=abc');
    await userEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(screen.getByText('bad cookie')).toBeInTheDocument());
    expect(input).toHaveValue('ezproxy=abc'); // nothing was saved — don't discard it
    expect(screen.getByText('not set')).toBeInTheDocument();
  });

  it('a successful save refreshes the shared settings store and the badge', async () => {
    stubSettingsFetch();
    render(ImportModal);
    await userEvent.click(screen.getByText(/institutional access/i));
    await userEvent.type(screen.getByPlaceholderText(/ezproxy/), 'ezproxy=abc');
    await userEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(screen.getByText('set')).toBeInTheDocument());
    expect(appSettings.proxyCookieSet).toBe(true);
  });

  it('Clear removes the cookie and surfaces the refreshed state', async () => {
    appSettings.proxyCookieSet = true;
    appSettings.proxyCookieUpdatedAt = '2026-08-14';
    stubSettingsFetch();
    render(ImportModal);
    await userEvent.click(screen.getByText(/institutional access/i));
    await userEvent.click(screen.getByRole('button', { name: 'Clear' }));
    await waitFor(() => expect(appSettings.proxyCookieSet).toBe(false));
    expect(screen.getByText('not set')).toBeInTheDocument();
  });

  it('names the configured proxy host in the help copy', async () => {
    stubSettingsFetch();
    render(ImportModal);
    await userEvent.click(screen.getByText(/institutional access/i));
    expect(screen.getByText('proxy.example.edu')).toBeInTheDocument();
  });

  it('hides institutional access entirely when no proxy is configured', async () => {
    appSettings.proxyHost = null;
    stubSettingsFetch({ noProxy: true });
    render(ImportModal);
    await waitFor(() => expect(screen.queryByText(/institutional access/i)).toBeNull());
  });
});
