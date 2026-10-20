<script lang="ts">
  import { X } from 'lucide-svelte';
  import type { Snippet } from 'svelte';
  import { fade, scale } from 'svelte/transition';
  import { DUR, dur } from './motion';

  let {
    title,
    onclose,
    children,
    footer,
  }: { title: string; onclose: () => void; children: Snippet; footer?: Snippet } = $props();

  let panel = $state<HTMLElement | null>(null);

  // Move focus into the dialog; hand it back when the dialog unmounts.
  $effect(() => {
    const previous = document.activeElement as HTMLElement | null;
    panel?.focus();
    return () => previous?.focus?.();
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      // Close on Esc. stopPropagation shields DOM-level handlers; the global
      // shortcut handler independently ignores keys while a modal is open.
      e.stopPropagation();
      onclose();
    } else if (e.key === 'Tab' && panel) {
      const focusables = Array.from(
        panel.querySelectorAll<HTMLElement>(
          'a[href], button:not([disabled]), input, select, textarea, summary, [tabindex]:not([tabindex="-1"])',
        ),
      ).filter((el) => el.checkVisibility?.() ?? el.offsetParent !== null);
      if (focusables.length === 0) return;
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
  }
</script>

<svelte:window onkeydown={onkeydown} />

<div
  transition:fade={{ duration: dur(DUR.fast) }}
  role="presentation"
  onclick={(e) => {
    if (e.target === e.currentTarget) onclose();
  }}
  class="fixed inset-0 z-50 flex items-center justify-center bg-stone-950/40 p-4 backdrop-blur-[2px]"
>
  <div
    bind:this={panel}
    tabindex="-1"
    transition:scale={{ start: 0.96, duration: dur(DUR.base) }}
    role="dialog"
    aria-modal="true"
    aria-label={title}
    class="flex max-h-[80vh] w-full max-w-lg flex-col rounded-xl border border-stone-200 bg-paper text-ink shadow-2xl outline-none dark:border-stone-800 dark:bg-soot dark:text-stone-100"
  >
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
</div>
