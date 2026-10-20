<script lang="ts">
  import { Download, ExternalLink, BookOpen } from 'lucide-svelte';
  import {
    citationHover,
    cancelHideCitation,
    hideCitationNow,
    hideCitationSoon,
  } from './state.svelte';
  import { enqueueUrl, openImport } from '$features/importing/queue.svelte';
  import { openTab } from '$features/reader/tabs/state.svelte';
  import { abbreviateVenue } from '$features/library/venue';
  import { anchoredPosition } from '$shared/ui/popoverPosition';
  import { authorLine, refLinks, titleCase } from './refFormat';

  const c = $derived(citationHover.current);
  const s = $derived(c?.reference.structured ?? null);
  const links = $derived(c ? refLinks(s, c.reference.externalUrl) : []);

  // Keep the popover on screen (shared flip/clamp policy — see popoverPosition).
  const pos = $derived(
    anchoredPosition(c?.screenX ?? 0, c?.screenY ?? 0, { maxW: 384 /* max-w-sm */, belowOffset: 18 }),
  );

  function open() {
    if (c?.matchedPaper) openTab(c.matchedPaper);
    hideCitationNow();
  }

  // A reference we can fetch but don't have: offer one-click import. The
  // import queue accepts a bare DOI or arXiv id (same as the Import dialog).
  const importable = $derived(!c?.matchedPaper && (s?.doi || s?.arxiv_id) ? (s?.doi ?? s?.arxiv_id) : null);

  function importRef() {
    if (!importable) return;
    openImport(); // reset/open first, so the enqueue lands in the fresh session
    void enqueueUrl(importable);
    hideCitationNow();
  }
</script>

{#if c}
  <div
    role="tooltip"
    onpointerenter={cancelHideCitation}
    onpointerleave={hideCitationSoon}
    style:left="{pos.left}px"
    style:top="{pos.top}px"
    style:transform={pos.below ? 'none' : 'translateY(-100%)'}
    class="pointer-events-auto fixed z-50 max-w-sm rounded-xl border border-stone-200 bg-paper p-3 text-[12.5px] shadow-2xl dark:border-stone-800 dark:bg-soot"
  >
    {#if s?.title}
      <p class="font-medium leading-snug text-ink dark:text-stone-100">{titleCase(s.title)}</p>
      {#if s.authors.length > 0}
        <p class="mt-0.5 leading-snug text-stone-500 dark:text-stone-400">{authorLine(s.authors)}</p>
      {/if}
      {#if s.venue || s.year}
        <p class="mt-0.5 text-xs text-stone-500 dark:text-stone-400">
          {[abbreviateVenue(s.venue), s.year].filter(Boolean).join(' · ')}
        </p>
      {/if}
    {:else}
      <p class="font-serif leading-relaxed text-stone-700 dark:text-stone-300">{c.reference.rawText}</p>
    {/if}
    <div class="mt-2 flex items-center gap-3">
      {#if c.matchedPaper}
        <button
          type="button"
          onclick={open}
          class="inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1 text-xs font-medium text-amber-700 hover:bg-amber-700/10 dark:border-stone-700 dark:text-amber-500"
        >
          <BookOpen size={13} /> Open in library
        </button>
      {/if}
      {#if importable}
        <button
          type="button"
          onclick={importRef}
          class="inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1 text-xs font-medium text-amber-700 hover:bg-amber-700/10 dark:border-stone-700 dark:text-amber-500"
        >
          <Download size={13} /> Import
        </button>
      {/if}
      {#each links as l (l.href)}
        <a
          href={l.href}
          target="_blank"
          rel="noopener noreferrer"
          class="inline-flex items-center gap-1 text-xs text-stone-500 hover:text-ink dark:text-stone-400"
        >
          <ExternalLink size={12} /> {l.label}
        </a>
      {/each}
    </div>
  </div>
{/if}
