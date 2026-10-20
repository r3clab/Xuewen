<script lang="ts">
  import { LibraryBig, X } from 'lucide-svelte';
  import { flip } from 'svelte/animate';
  import { crossfade, fade } from 'svelte/transition';
  import { DUR, dur } from '$shared/ui/motion';
  import { activateTab, closeTab, goHome, viewer } from './state.svelte';

  // The active-tab underline crossfades between tabs — a real sliding
  // indicator with no measurement code.
  const [send, receive] = crossfade({ duration: dur(DUR.fast) });
</script>

<div class="flex h-9 shrink-0 items-center border-b border-stone-200 bg-paper dark:border-stone-800 dark:bg-night">
  <button
    type="button"
    aria-label="Library"
    aria-current={viewer.activeId === null ? 'page' : undefined}
    onclick={goHome}
    class={`relative flex h-9 shrink-0 items-center gap-1.5 px-3 text-sm ${
      viewer.activeId === null
        ? 'text-ink dark:text-stone-100'
        : 'text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800/40'
    }`}
  >
    <LibraryBig size={15} />
    Library
    {#if viewer.activeId === null}
      <span
        in:receive={{ key: 'tab-underline' }}
        out:send={{ key: 'tab-underline' }}
        class="absolute inset-x-2 top-0 h-0.5 rounded-full bg-amber-700 dark:bg-amber-500"
      ></span>
    {/if}
  </button>
  <span class="h-5 w-px shrink-0 bg-stone-200 dark:bg-stone-800"></span>

  <div class="flex min-w-0 flex-1 items-center overflow-x-auto">
    {#each viewer.tabs as tab (tab.id)}
      <div
        animate:flip={{ duration: dur(DUR.base) }}
        out:fade={{ duration: dur(DUR.fast) }}
        class={`group relative flex h-9 max-w-32 shrink-0 items-center gap-1 border-r border-stone-200 px-0.5 dark:border-stone-800 ${
          viewer.activeId === tab.id
            ? 'bg-parchment/70 dark:bg-stone-800/60'
            : 'hover:bg-parchment/50 dark:hover:bg-stone-800/30'
        }`}
      >
        <!-- A named paper labels its tab with the name: at max-w-32 a long
             title truncates to a few useless words, while "RVSpec" is the
             whole handle and fits. Sans semibold matches the library table's
             Name column — a name reads as a name wherever it appears — and
             unnamed tabs keep the serif title. Either way the tooltip is the
             full title, which is the only place it can still be read.

             The label is centered against a spacer the same width as the
             close button, so the button's reserved box — it is invisible
             until hover, but always occupies layout — cannot pull the label
             off-center. The button is a square flex box rather than a bare
             icon, or its hover fill wraps the glyph's own bounds and sits
             visibly off-center inside the tab. -->
        <span class="w-4 shrink-0" aria-hidden="true"></span>
        <button
          type="button"
          title={tab.title}
          onclick={() => activateTab(tab.id)}
          class={`min-w-0 truncate text-stone-700 dark:text-stone-200 ${
            tab.name ? 'font-sans text-xs font-semibold' : 'font-serif text-sm'
          }`}
        >
          {tab.name ?? tab.title}
        </button>
        <button
          type="button"
          aria-label="Close tab"
          onclick={() => closeTab(tab.id)}
          class="flex h-4 w-4 shrink-0 items-center justify-center rounded-sm text-stone-500 opacity-0 hover:bg-stone-200 focus-visible:opacity-100 group-hover:opacity-100 dark:text-stone-400 dark:hover:bg-stone-700"
        >
          <X size={14} />
        </button>
        {#if viewer.activeId === tab.id}
          <span
            in:receive={{ key: 'tab-underline' }}
            out:send={{ key: 'tab-underline' }}
            class="absolute inset-x-2 top-0 h-0.5 rounded-full bg-amber-700 dark:bg-amber-500"
          ></span>
        {/if}
      </div>
    {/each}
  </div>

</div>
