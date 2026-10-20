<script lang="ts">
  import { fade, fly } from 'svelte/transition';
  import { getPreviewMeta, listPapers, previewPageUrl } from '$features/library/api';
  import { fuzzyScore } from './fuzzy';
  import { library } from '$features/library/state.svelte';
  import { finder } from './state.svelte';
  import { DUR, dur } from '$shared/ui/motion';
  import { openTab } from '$features/reader/tabs/state.svelte';
  import type { PaperSummary } from '$shared/api/generated/PaperSummary';
  import type { PreviewMeta } from '$shared/api/generated/PreviewMeta';
  import PreviewFallbackCard from './PreviewFallbackCard.svelte';

  let query = $state('');
  let active = $state(0);
  let input = $state<HTMLInputElement | null>(null);

  $effect(() => {
    input?.focus();
  });

  // The picker jumps anywhere in the library, so it can't read the table's
  // view — `library.papers` is whatever the current filter or search left
  // there. Fetched fresh per open (the component is `{#if}`-gated, so mount
  // == open); until it resolves — or if it fails — the filtered view stands
  // in, which is still better than an empty list.
  let corpus = $state<PaperSummary[] | null>(null);
  $effect(() => {
    void listPapers({ q: '', status: 'all', sort: 'added_desc', project: 'all' })
      .then((papers) => (corpus = papers))
      .catch(() => {});
  });

  // Name and title only. Authors and cite keys are deliberately not matched:
  // every match is then visible in the row that matched it.
  const matches = $derived.by(() => {
    const q = query.trim();
    return (corpus ?? library.papers)
      .map((p) => ({ p, score: fuzzyScore(q, `${p.name ?? ''} ${p.title ?? ''}`.trim()) }))
      .filter((x): x is { p: PaperSummary; score: number } => x.score !== null)
      .sort((a, b) => b.score - a.score);
  });

  // Clamped rather than reset: the corpus lands while the picker is already
  // open, and snapping the highlight home on that would move the row out from
  // under an Enter the user is mid-way through pressing.
  const index = $derived(Math.min(active, matches.length - 1));
  const highlighted = $derived(matches[index]?.p ?? null);

  $effect(() => {
    void query;
    active = 0;
  });

  // Preview state for whichever row is highlighted. `meta === null` with no
  // error means "still loading"; a rejected fetch (422 for an unrenderable
  // PDF, 404 for a missing file, or a network failure) means the fallback
  // card, which needs nothing but the row already in hand.
  let meta = $state<PreviewMeta | null>(null);
  let previewFailed = $state(false);

  $effect(() => {
    const paper = highlighted;
    if (!paper) return;
    meta = null;
    previewFailed = false;
    // Arrowing through a list outruns the network; this drops every answer
    // but the last row's, so a fast scan can't paint a stale document.
    let cancelled = false;
    const timer = setTimeout(() => {
      void getPreviewMeta(paper.id)
        .then((m) => {
          if (!cancelled) meta = m;
        })
        .catch(() => {
          if (!cancelled) previewFailed = true;
        });
    }, 120);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  });

  function close() {
    finder.open = false;
  }

  function open(paper: PaperSummary) {
    close();
    openTab(paper);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      // Stop here: the global handler's Esc cascade would otherwise go on to
      // close the dock or leave zen behind the overlay.
      e.stopPropagation();
      close();
    } else if (e.key === 'Tab') {
      // Single-focus surface: rows are tabindex="-1" and the page beneath is
      // not inert, so Tab must not carry focus out.
      e.preventDefault();
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      active = Math.min(matches.length - 1, index + 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      active = Math.max(0, index - 1);
    } else if (e.key === 'Enter' && highlighted) {
      e.preventDefault();
      open(highlighted);
    }
  }

  // Keep the highlighted row in view when the arrow keys walk past the edge.
  function scrollRowIntoView(el: HTMLElement, isActive: boolean) {
    if (isActive) el.scrollIntoView({ block: 'nearest' });
    return {
      update(nowActive: boolean) {
        if (nowActive) el.scrollIntoView({ block: 'nearest' });
      },
    };
  }
</script>

<div
  transition:fade={{ duration: dur(DUR.fast) }}
  role="presentation"
  onclick={(e) => {
    if (e.target === e.currentTarget) close();
  }}
  class="fixed inset-0 z-[60] flex items-start justify-center bg-stone-950/40 p-4 pt-[10vh] backdrop-blur-[2px]"
>
  <div
    transition:fly={{ y: -12, duration: dur(DUR.base) }}
    role="dialog"
    aria-modal="true"
    aria-label="Find paper"
    class="flex max-h-full w-full max-w-3xl flex-col overflow-hidden rounded-xl border border-stone-200 bg-paper shadow-2xl dark:border-stone-800 dark:bg-soot"
  >
    <div class="flex shrink-0 items-center gap-2 border-b border-stone-200 px-3.5 dark:border-stone-800">
      <span aria-hidden="true" class="text-sm font-semibold text-amber-700 dark:text-amber-500">›</span>
      <input
        bind:this={input}
        bind:value={query}
        {onkeydown}
        role="combobox"
        aria-expanded="true"
        aria-controls="picker-list"
        aria-label="Fuzzy-match a name or title"
        aria-activedescendant={matches[index] ? `picker-opt-${index}` : undefined}
        placeholder="Fuzzy-match a name or title…"
        class="w-full bg-transparent py-3 text-sm text-ink outline-none dark:text-stone-100"
      />
      <span class="shrink-0 text-caption tabular-nums text-stone-400 dark:text-stone-500">
        {matches.length} / {(corpus ?? library.papers).length}
      </span>
    </div>

    <div class="flex min-h-0 flex-1">
      <ul
        id="picker-list"
        role="listbox"
        aria-label="Papers"
        class="flex h-[26rem] w-[55%] flex-col gap-0.5 overflow-y-auto border-r border-stone-200 p-1.5 dark:border-stone-800"
      >
        {#if matches.length === 0}
          <li class="px-3 py-6 text-center text-sm text-stone-500 dark:text-stone-400">
            Nothing matches. Try fewer letters.
          </li>
        {/if}
        {#each matches as { p }, i (p.id)}
          <li id={`picker-opt-${i}`} role="option" aria-selected={i === index}>
            <button
              type="button"
              tabindex="-1"
              use:scrollRowIntoView={i === index}
              onclick={() => open(p)}
              onmousemove={() => (active = i)}
              class={`flex w-full flex-col gap-0.5 rounded-lg px-2.5 py-2 text-left ${
                i === index ? 'bg-amber-700/10 dark:bg-amber-500/10' : ''
              }`}
            >
              <span class="flex items-start gap-2">
                <span class="min-w-0 flex-1 font-serif text-sm leading-snug text-ink dark:text-stone-100">
                  {#if p.name}<span class="mr-1.5 font-sans text-xs font-semibold text-amber-700 dark:text-amber-500"
                      >{p.name}</span
                    >{/if}{p.title ?? '(untitled)'}
                </span>
                {#if p.status === 'needs_review'}
                  <span
                    class="shrink-0 rounded border border-stone-200 bg-parchment px-1.5 text-chip uppercase tracking-wide text-stone-500 dark:border-stone-700 dark:bg-stone-800 dark:text-stone-400"
                  >
                    needs review
                  </span>
                {/if}
              </span>
              <!-- Disambiguation only — none of this is matched against. -->
              <span class="truncate text-detail text-stone-500 dark:text-stone-400">
                {[
                  p.authors.length > 2 ? `${p.authors[0]} … ${p.authors.at(-1)}` : p.authors.join(', '),
                  p.venue,
                  p.year,
                ]
                  .filter(Boolean)
                  .join(' · ')}
              </span>
            </button>
          </li>
        {/each}
      </ul>

      <div
        class="flex h-[26rem] w-[45%] flex-col items-center gap-2.5 overflow-y-auto bg-parchment p-3 dark:bg-stone-900/40"
      >
        {#if highlighted && previewFailed}
          <PreviewFallbackCard paper={highlighted} />
        {:else if highlighted && meta}
          {#each { length: meta.pages } as _, page (page)}
            <div class="flex w-full flex-col items-center gap-1">
              <img
                src={previewPageUrl(highlighted.id, page)}
                alt={`Page ${page + 1}`}
                width={meta.page_width}
                height={meta.page_height}
                loading={page === 0 ? 'eager' : 'lazy'}
                decoding="async"
                class="w-full max-w-[250px] rounded-[2px] border border-stone-300 bg-white dark:border-stone-700"
              />
              <span class="text-chip tabular-nums text-stone-400 dark:text-stone-500">{page + 1}</span>
            </div>
          {/each}
        {/if}
      </div>
    </div>

    <div
      class="flex shrink-0 items-center gap-4 border-t border-stone-200 bg-parchment px-3.5 py-1.5 text-caption text-stone-500 dark:border-stone-800 dark:bg-stone-900/40 dark:text-stone-400"
    >
      <span>↑↓ move</span>
      <span>⏎ open as tab</span>
      <span>esc close</span>
      <span class="ml-auto">matching name + title</span>
    </div>
  </div>
</div>
