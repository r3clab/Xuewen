<script lang="ts">
  import { ArchiveArrowUp, Check, CircleExclamationPoint, Copy, FileExclamationPoint, Link, Loader, Trash, Upload } from '@lucide/svelte';
  import { clearProxyCookie, setProxyCookie } from './api';
  import {
    closeImport,
    enqueueFiles,
    enqueueUrl,
    importState,
    restoreTrashed,
    type ImportState,
  } from './queue.svelte';
  import { appSettings, loadSettings } from '$shared/api/settings.svelte';
  import { errorMessage } from '$shared/errors';
  import Modal from '$shared/ui/Modal.svelte';

  let dragging = $state(false);
  let input: HTMLInputElement;

  function pick(list: FileList | null) {
    if (!list) return;
    const files = Array.from(list).filter(
      (f) => /\.pdf$/i.test(f.name) || f.type === 'application/pdf',
    );
    if (files.length) void enqueueFiles(files);
  }

  function onDrop(e: DragEvent) {
    e.preventDefault();
    dragging = false;
    pick(e.dataTransfer?.files ?? null);
  }

  let urlInput = $state('');
  function submitUrl() {
    const v = urlInput.trim();
    if (!v) return;
    urlInput = '';
    void enqueueUrl(v);
  }

  // The cookie state lives in the shared appSettings store (the app-startup
  // loadSettings and this modal read the same `/api/settings`); the modal
  // only re-calls loadSettings after mutating the cookie. Failures surface
  // inline (the DockCode attach/detach pattern) — a rejected save must not
  // leave the "not set" badge silently standing.
  let cookieInput = $state('');
  let cookieBusy = $state(false);
  let cookieError = $state<string | null>(null);
  async function saveCookie() {
    const v = cookieInput.trim();
    if (!v || cookieBusy) return;
    cookieBusy = true;
    cookieError = null;
    try {
      await setProxyCookie(v);
      cookieInput = '';
      await loadSettings();
    } catch (e) {
      cookieError = errorMessage(e, 'cookie update failed');
    } finally {
      cookieBusy = false;
    }
  }
  async function removeCookie() {
    if (cookieBusy) return;
    cookieBusy = true;
    cookieError = null;
    try {
      await clearProxyCookie();
      await loadSettings();
    } catch (e) {
      cookieError = errorMessage(e, 'cookie update failed');
    } finally {
      cookieBusy = false;
    }
  }
  // Refresh once when the modal mounts (the cookie may have expired or been
  // set from the CLI since startup).
  $effect(() => {
    void loadSettings();
  });

  // Icon and summary bucket per outcome; the Record type makes a new outcome
  // a type error here instead of a silent fall-through to the queued styling.
  const ROW_STYLE: Record<
    ImportState['outcome'],
    { icon: typeof Check | null; iconClass: string; bucket: 'ingested' | 'skipped' | 'failed' | 'pending' }
  > = {
    queued: { icon: null, iconClass: '', bucket: 'pending' },
    importing: { icon: Loader, iconClass: 'shrink-0 animate-spin text-amber-600', bucket: 'pending' },
    ingested: { icon: Check, iconClass: 'shrink-0 text-lime-600', bucket: 'ingested' },
    duplicate: { icon: Copy, iconClass: 'shrink-0 text-stone-400', bucket: 'skipped' },
    same_work: { icon: Copy, iconClass: 'shrink-0 text-stone-400', bucket: 'skipped' },
    in_trash: { icon: Trash, iconClass: 'shrink-0 text-stone-400', bucket: 'skipped' },
    restored: { icon: ArchiveArrowUp, iconClass: 'shrink-0 text-lime-600', bucket: 'skipped' },
    unfetched: { icon: FileExclamationPoint, iconClass: 'shrink-0 text-yellow-600', bucket: 'skipped' },
    failed: { icon: CircleExclamationPoint, iconClass: 'shrink-0 text-red-500', bucket: 'failed' },
  };

  function detail(state: ImportState): string {
    switch (state.outcome) {
      case 'queued':
        return 'queued';
      case 'importing':
        return 'importing…';
      case 'ingested':
        return state.title ?? '(untitled)';
      case 'duplicate':
        return 'duplicate';
      case 'same_work':
        return 'already in library';
      case 'in_trash':
        return 'in trash';
      case 'restored':
        return 'restored from trash';
      case 'unfetched':
        return `no PDF — download & drop in inbox: ${state.title ?? '(untitled)'}`;
      case 'failed':
        return state.message;
    }
  }

  const summary = $derived.by(() => {
    const c = { ingested: 0, skipped: 0, failed: 0 };
    for (const i of importState.items) {
      const bucket = ROW_STYLE[i.state.outcome].bucket;
      if (bucket !== 'pending') c[bucket]++;
    }
    return c;
  });
</script>

{#snippet importFooter()}
  <p class="text-xs text-stone-500 dark:text-stone-400">
    {summary.ingested} ingested, {summary.skipped} skipped, {summary.failed} failed
  </p>
{/snippet}

<Modal
  title="Import papers"
  onclose={closeImport}
  footer={importState.items.length ? importFooter : undefined}
>
  <form
    class="mb-3 flex gap-2"
    onsubmit={(e) => {
      e.preventDefault();
      submitUrl();
    }}
  >
    <div class="flex flex-1 items-center gap-2 rounded-lg border border-stone-300 px-2 dark:border-stone-700">
      <Link size={16} class="shrink-0 text-stone-400" />
      <input
        bind:value={urlInput}
        type="text"
        placeholder="Paste a link, DOI, or arXiv id"
        class="w-full bg-transparent py-2 text-sm outline-none"
      />
    </div>
    <button
      type="submit"
      class="rounded-lg bg-amber-700 px-3 py-2 text-sm font-medium text-white hover:bg-amber-800 disabled:opacity-50 dark:bg-amber-600 dark:hover:bg-amber-500"
      disabled={!urlInput.trim()}
    >
      Add
    </button>
  </form>
  <button
    type="button"
    onclick={() => input.click()}
    ondragover={(e) => {
      e.preventDefault();
      dragging = true;
    }}
    ondragleave={() => (dragging = false)}
    ondrop={onDrop}
    class="flex w-full flex-col items-center gap-2 rounded-xl border-2 border-dashed p-8 text-sm transition-colors {dragging
      ? 'border-amber-600 bg-amber-700/5 dark:bg-amber-500/10'
      : 'border-stone-300 dark:border-stone-700'}"
  >
    <Upload size={24} class="pointer-events-none text-stone-400" />
    <span class="pointer-events-none text-stone-600 dark:text-stone-300">Drag PDFs here, or click to browse</span>
  </button>
  <input
    bind:this={input}
    type="file"
    accept=".pdf,application/pdf"
    multiple
    class="hidden"
    onchange={(e) => pick((e.currentTarget as HTMLInputElement).files)}
  />

  {#if importState.items.length}
    <ul class="mt-4 space-y-1">
      {#each importState.items as item, i (i)}
        {@const progress = item.state}
        {@const look = ROW_STYLE[progress.outcome]}
        <li class="flex items-center gap-2 rounded-lg px-2 py-1.5 text-sm">
          {#if look.icon}
            <look.icon size={14} class={look.iconClass} />
          {:else}
            <span class="h-3.5 w-3.5 shrink-0 rounded-full border border-stone-300 dark:border-stone-600"></span>
          {/if}
          <span class="min-w-0 flex-1 truncate text-stone-700 dark:text-stone-200">{item.name}</span>
          <span class="flex max-w-[55%] shrink-0 items-center justify-end gap-1.5 text-xs">
            {#if progress.outcome === 'ingested' && progress.status === 'needs_review'}
              <span class="shrink-0 rounded bg-yellow-100 px-1.5 py-0.5 font-medium text-yellow-700 dark:bg-yellow-500/15 dark:text-yellow-400">needs review</span>
            {/if}
            <span class="truncate text-stone-500 dark:text-stone-400" title={detail(progress)}>{detail(progress)}</span>
            {#if progress.outcome === 'in_trash'}
              <button
                type="button"
                onclick={() => void restoreTrashed(item)}
                class="shrink-0 rounded px-1.5 py-0.5 font-medium text-amber-700 hover:bg-amber-700/10 dark:text-amber-400 dark:hover:bg-amber-400/10"
              >Restore</button>
            {/if}
          </span>
        </li>
      {/each}
    </ul>
  {/if}

  <!-- Hidden entirely without a configured [proxy]: a cookie saved on such a
       deployment is never used by the import fetcher, so offering the form
       would only mislead. The host comes from the server's own config. -->
  {#if appSettings.proxyHost}
    <details class="mt-4 rounded-lg border border-stone-200 text-sm dark:border-stone-800">
      <summary class="cursor-pointer px-3 py-2 text-stone-600 dark:text-stone-300">
        Institutional access (EZproxy cookie)
        {#if appSettings.proxyCookieSet}
          <span class="ml-1 rounded bg-lime-100 px-1.5 py-0.5 text-xs text-lime-700 dark:bg-lime-500/15 dark:text-lime-400">set</span>
        {:else}
          <span class="ml-1 rounded bg-stone-100 px-1.5 py-0.5 text-xs text-stone-500 dark:bg-stone-800 dark:text-stone-400">not set</span>
        {/if}
      </summary>
      <div class="space-y-2 border-t border-stone-200 p-3 dark:border-stone-800">
        <p class="text-xs text-stone-500 dark:text-stone-400">
          Paste the <code>Cookie:</code> header for <code>{appSettings.proxyHost}</code> (from a browser
          cookie extension or DevTools) to fetch paywalled ACM/IEEE PDFs. It expires — refresh it here.
        </p>
        <div class="flex gap-2">
          <input
            bind:value={cookieInput}
            type="password"
            placeholder="ezproxy=…; …"
            class="w-full rounded-lg border border-stone-300 bg-transparent px-2 py-1.5 text-sm outline-none dark:border-stone-700"
          />
          <button
            type="button"
            onclick={saveCookie}
            disabled={!cookieInput.trim() || cookieBusy}
            class="rounded-lg bg-stone-700 px-3 py-1.5 text-sm text-white hover:bg-stone-600 disabled:opacity-50"
          >Save</button>
        </div>
        {#if cookieError}
          <p class="text-xs text-red-600 dark:text-red-400">{cookieError}</p>
        {/if}
        {#if appSettings.proxyCookieSet}
          <div class="flex items-center justify-between text-xs text-stone-500 dark:text-stone-400">
            <span>Updated {appSettings.proxyCookieUpdatedAt ?? '—'}</span>
            <button
              type="button"
              onclick={removeCookie}
              disabled={cookieBusy}
              class="text-red-500 hover:underline disabled:opacity-50"
            >Clear</button>
          </div>
        {/if}
      </div>
    </details>
  {/if}
</Modal>
