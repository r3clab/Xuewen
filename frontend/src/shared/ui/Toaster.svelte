<script lang="ts">
  import { CircleAlert, CircleCheck, Info, X } from 'lucide-svelte';
  import { fade, fly } from 'svelte/transition';
  import { DUR, dur } from './motion';
  import { dismissToast, toasts } from './toasts.svelte';
</script>

<div
  class="pointer-events-none fixed bottom-4 left-4 z-[70] flex w-80 flex-col gap-2"
  role="status"
  aria-live="polite"
>
  {#each toasts.items as t (t.id)}
    <div
      in:fly={{ y: 16, duration: dur(DUR.base) }}
      out:fade={{ duration: dur(DUR.fast) }}
      role={t.kind === 'error' ? 'alert' : undefined}
      class="pointer-events-auto flex items-center gap-2 rounded-lg border border-stone-200 bg-paper px-3 py-2 text-sm text-ink shadow-lg dark:border-stone-800 dark:bg-soot dark:text-stone-100"
    >
      {#if t.kind === 'success'}
        <CircleCheck size={16} class="shrink-0 text-lime-700 dark:text-lime-400" />
      {:else if t.kind === 'error'}
        <CircleAlert size={16} class="shrink-0 text-red-600 dark:text-red-400" />
      {:else}
        <Info size={16} class="shrink-0 text-stone-500 dark:text-stone-400" />
      {/if}
      <span class="min-w-0 flex-1">{t.message}</span>
      {#if t.action}
        {@const action = t.action}
        <button
          type="button"
          onclick={() => {
            action.run();
            dismissToast(t.id);
          }}
          class="shrink-0 rounded px-1.5 py-0.5 text-xs font-semibold text-amber-700 hover:bg-amber-700/10 dark:text-amber-500 dark:hover:bg-amber-500/15"
        >{action.label}</button>
      {/if}
      <button
        type="button"
        aria-label="Dismiss"
        onclick={() => dismissToast(t.id)}
        class="rounded p-0.5 text-stone-400 hover:bg-parchment dark:hover:bg-stone-800"
      >
        <X size={14} />
      </button>
    </div>
  {/each}
</div>
