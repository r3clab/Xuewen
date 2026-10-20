<script lang="ts">
  import { fade } from 'svelte/transition';
  import { chordKeyLabel } from '$app/keys/keymap';
  import { leader, leaderContinuations } from '$app/keys/leader.svelte';
  import { DUR, dur } from '$shared/ui/motion';

  // Helix's which-key: the discovery affordance that replaces ⌘K's browsable
  // action list. It reads the same table the dispatcher runs, so a chord can
  // never be bound but unlisted.
  const continuations = $derived(leaderContinuations());
</script>

{#if leader.pending.length > 0}
  <div
    transition:fade={{ duration: dur(DUR.fast) }}
    role="status"
    aria-live="polite"
    class="fixed bottom-4 left-4 z-[60] flex flex-col gap-1.5 rounded-lg border border-stone-300 bg-paper px-3 py-2 shadow-lg dark:border-stone-700 dark:bg-soot"
  >
    <div class="text-caption font-semibold uppercase tracking-wider text-stone-400 dark:text-stone-500">
      {chordKeyLabel(leader.pending)}
    </div>
    {#each continuations as chord (chord.keys.join(' '))}
      <div class="flex items-baseline gap-2 text-detail">
        <kbd
          class="rounded border border-stone-300 px-1.5 font-sans text-caption font-semibold text-ink dark:border-stone-700 dark:text-stone-100"
        >
          {chordKeyLabel(chord.keys.slice(leader.pending.length))}
        </kbd>
        <span class="text-stone-500 dark:text-stone-400">{chord.label}</span>
      </div>
    {/each}
  </div>
{/if}
