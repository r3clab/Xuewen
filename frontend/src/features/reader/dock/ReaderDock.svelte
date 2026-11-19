<script lang="ts">
  import { Maximize2, Minimize2, X } from '@lucide/svelte';
  import type { Attachment } from 'svelte/attachments';
  import { fly } from 'svelte/transition';
  import { chat } from '$features/reader/ask/chat.svelte';
  import { DUR, dur } from '$shared/ui/motion';
  import { toggleZen, viewer } from '$features/reader/tabs/state.svelte';
  import { closeDock } from './state.svelte';
  import DockComposer from '$features/reader/ask/DockComposer.svelte';
  import DockDetails from './DockDetails.svelte';
  import DockThread from '$features/reader/ask/DockThread.svelte';

  let { id }: { id: string } = $props();

  let scroller: HTMLElement;

  // The record sits above the thread in one scroll, so following the answer
  // is opt-in: it starts off (opening the dock must land on the record, not
  // jump past it to an old conversation) and turns on when the reader asks
  // something or scrolls to the bottom themselves.
  let stick = false;
  function onScroll() {
    stick = scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 40;
  }
  const followGrowth: Attachment<HTMLElement> = (content) => {
    const observer = new ResizeObserver(() => {
      if (stick) scroller.scrollTop = scroller.scrollHeight;
    });
    observer.observe(content);
    return () => observer.disconnect();
  };
</script>

<aside
  transition:fly={{ x: 24, duration: dur(DUR.base) }}
  aria-label="Paper panel"
  class="absolute inset-y-3 right-3 z-40 flex w-96 max-w-[calc(100%-1.5rem)] flex-col overflow-hidden rounded-2xl border border-stone-200 bg-paper shadow-2xl dark:border-stone-800 dark:bg-soot"
>
  <!-- Close/zen sit on the LEFT: the quick-action rail (問, top-right of the
       reader) hides when the dock opens, and a follow-up click at its old
       position must not land on close, which would instantly dismiss the
       panel that click just opened. -->
  <div class="flex shrink-0 items-center gap-0.5 border-b border-stone-200 px-2.5 py-2 dark:border-stone-800">
    <button
      type="button"
      class="rounded-lg p-1.5 text-stone-500 hover:bg-parchment hover:text-ink dark:text-stone-400 dark:hover:bg-stone-800 dark:hover:text-stone-100"
      aria-label="Close panel"
      title="Close — Esc"
      onclick={closeDock}
    >
      <X size={15} />
    </button>
    <button
      type="button"
      class="rounded-lg p-1.5 text-stone-500 hover:bg-parchment hover:text-ink dark:text-stone-400 dark:hover:bg-stone-800 dark:hover:text-stone-100"
      aria-label="Zen mode"
      title="Zen — z"
      onclick={toggleZen}
    >
      {#if viewer.zen}<Minimize2 size={15} />{:else}<Maximize2 size={15} />{/if}
    </button>
  </div>

  <div bind:this={scroller} onscroll={onScroll} class="min-h-0 flex-1 overflow-y-auto px-4 py-4">
    <div {@attach followGrowth}>
      <DockDetails {id} />
      {#if chat.available}
        <DockThread />
      {/if}
    </div>
  </div>

  {#if chat.available}
    <DockComposer onsend={() => (stick = true)} />
  {/if}
</aside>
