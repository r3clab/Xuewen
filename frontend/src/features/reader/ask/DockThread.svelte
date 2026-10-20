<script lang="ts">
  import { chat } from './chat.svelte';
  import type { ToolCall } from '$shared/api/generated/ToolCall';

  // An untouched paper renders nothing at all: the dock is then exactly the
  // record it used to be, plus the composer at its foot.
  const shown = $derived(
    chat.messages.length > 0 || chat.pending !== null || chat.error !== null,
  );
</script>

{#snippet toolChips(tools: ToolCall[])}
  <div class="mb-1 flex flex-wrap gap-1">
    {#each tools as t, i (i)}
      <span class="rounded-md bg-amber-700/10 px-1.5 py-0.5 font-mono text-chip text-amber-700 dark:bg-amber-500/15 dark:text-amber-500">
        {t.name}{t.detail ? ` ${t.detail}` : ''}
      </span>
    {/each}
  </div>
{/snippet}

{#if shown}
  <section class="mt-4 border-t border-stone-200 pt-4 dark:border-stone-800">
    <h3 class="text-caption font-semibold uppercase tracking-[.08em] text-stone-500 dark:text-stone-400">
      Conversation
    </h3>
    <div class="mt-2 space-y-3">
      {#each chat.messages as m (m.id)}
        {#if m.role === 'user'}
          <div class="ml-8 whitespace-pre-wrap rounded-lg bg-parchment px-3 py-2 text-sm text-ink dark:bg-stone-800 dark:text-stone-100">
            {m.content}
          </div>
        {:else}
          <div>
            {#if m.tools?.length}{@render toolChips(m.tools)}{/if}
            <div class="whitespace-pre-wrap font-serif text-lead leading-relaxed text-stone-700 dark:text-stone-300">
              {m.content}
            </div>
            {#if m.model}
              <p class="mt-1 font-mono text-chip uppercase tracking-wide text-stone-400 dark:text-stone-500">
                {m.model}
              </p>
            {/if}
          </div>
        {/if}
      {/each}
      {#if chat.pending !== null}
        <div class="ml-8 whitespace-pre-wrap rounded-lg bg-parchment px-3 py-2 text-sm text-ink dark:bg-stone-800 dark:text-stone-100">
          {chat.pending}
        </div>
        <div>
          {#if chat.streamTools.length}{@render toolChips(chat.streamTools)}{/if}
          <div class="whitespace-pre-wrap font-serif text-lead leading-relaxed text-stone-700 dark:text-stone-300">
            {chat.streaming}<span class="animate-pulse">▍</span>
          </div>
        </div>
      {/if}
      {#if chat.error}
        <p class="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700 dark:border-red-900/50 dark:bg-red-500/10 dark:text-red-400">
          {chat.error}
        </p>
      {/if}
    </div>
  </section>
{/if}
