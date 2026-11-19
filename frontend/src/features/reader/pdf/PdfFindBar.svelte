<script lang="ts">
  import { ChevronDown, ChevronUp, X } from '@lucide/svelte';
  import { useSearch, useSearchState } from '@embedpdf/svelte/search';
  import { btn } from './pillStyles';
  import { findFocus, setFind } from '$features/reader/state.svelte';
  import { viewer } from '$features/reader/tabs/state.svelte';
  import { escapeLayer } from '$shared/ui/escape';
  import { focusOnMount } from '$shared/ui/focus';

  let { documentId }: { documentId: string } = $props();
  const search = useSearch();
  const found = useSearchState();

  let query = $state('');
  let debounce: ReturnType<typeof setTimeout> | undefined;

  // On unmount stop the session so highlights clear.
  $effect(() => () => {
    clearTimeout(debounce);
    search.clear();
  });

  // A failed or refused search leaves the count at zero, which is all the
  // bar has to say about it.
  function run(): void {
    void search.search({ text: query.trim() }).catch(() => {});
  }
  function onInput(): void {
    clearTimeout(debounce);
    debounce = setTimeout(run, 250);
  }

  const count = $derived(found.hitCount);
  function next(): void {
    if (count) search.nextHit();
  }
  function prev(): void {
    if (count) search.previousHit();
  }
  function close(): void {
    setFind(documentId, false);
  }
  // Enter stays on the input only: Enter on a focused button must keep
  // activating that button, not cycle matches.
  function onKeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter') {
      e.preventDefault();
      if (e.shiftKey) prev();
      else next();
    }
  }
</script>

<!-- Hidden tabs keep their bar mounted; Esc belongs to the visible one. -->
<div
  {@attach escapeLayer(close, { active: () => viewer.activeId === documentId })}
  role="search"
  aria-label="Find in document"
  class="absolute left-1/2 top-14 z-20 flex -translate-x-1/2 items-center gap-1 rounded-xl border border-stone-200 bg-paper/90 px-1.5 py-1 shadow backdrop-blur dark:border-stone-800 dark:bg-soot/90"
>
  <input
    {@attach focusOnMount}
    {@attach findFocus(documentId).attach}
    bind:value={query}
    placeholder="Find in document"
    aria-label="Find in document"
    class="w-48 bg-transparent px-1 text-sm text-ink placeholder:text-stone-400 focus:outline-none dark:text-stone-100 dark:placeholder:text-stone-500"
    oninput={onInput}
    onkeydown={onKeydown}
  />
  <span class="min-w-12 text-center text-xs tabular-nums text-stone-500 dark:text-stone-400">
    {query.trim() ? `${count ? found.activeHitIndex + 1 : 0} / ${count}` : ''}
  </span>
  <button type="button" class={btn} aria-label="Previous match" disabled={!count} onclick={prev}>
    <ChevronUp size={16} />
  </button>
  <button type="button" class={btn} aria-label="Next match" disabled={!count} onclick={next}>
    <ChevronDown size={16} />
  </button>
  <button type="button" class={btn} aria-label="Close find" onclick={close}>
    <X size={16} />
  </button>
</div>
