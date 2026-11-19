<script lang="ts">
  import { X } from '@lucide/svelte';
  import type { Snippet } from 'svelte';
  import { scale } from 'svelte/transition';
  import { modalDialog } from './dialog';
  import { DUR, dur } from './motion';

  let {
    title,
    onclose,
    children,
    footer,
  }: { title: string; onclose: () => void; children: Snippet; footer?: Snippet } = $props();
</script>

<dialog
  {@attach modalDialog(() => onclose())}
  transition:scale={{ start: 0.96, duration: dur(DUR.base) }}
  aria-label={title}
  class="m-auto w-full max-w-lg rounded-xl border border-stone-200 bg-paper p-0 text-ink shadow-2xl backdrop:bg-stone-950/40 backdrop:backdrop-blur-[2px] dark:border-stone-800 dark:bg-soot dark:text-stone-100"
>
  <div class="flex max-h-[80vh] flex-col">
    <div class="flex items-center justify-between border-b border-stone-200 p-4 dark:border-stone-800">
      <h2 class="font-serif text-base font-semibold">{title}</h2>
      <button
        type="button"
        onclick={onclose}
        aria-label="Close dialog"
        class="rounded-lg p-1.5 text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
      >
        <X size={18} />
      </button>
    </div>
    <div class="min-h-0 flex-1 overflow-y-auto p-4">
      {@render children()}
    </div>
    {#if footer}
      <div class="border-t border-stone-200 p-3 dark:border-stone-800">
        {@render footer()}
      </div>
    {/if}
  </div>
</dialog>
