<script module lang="ts">
  /// The dock's single composer. `c` (and the rail's 問) focus it by id —
  /// one dock, one live instance, so an id is enough and no ref has to be
  /// threaded up through ReaderDock.
  export const COMPOSER_ID = 'dock-composer';
</script>

<script lang="ts">
  import { Eraser, SendHorizontal, Square } from 'lucide-svelte';
  import { chat, clearChatThread, sendChatMessage, setChatModel, stopChatStream } from './chat.svelte';
  import ConfirmButtons from '$shared/ui/ConfirmButtons.svelte';

  let confirmingClear = $state(false);

  function onComposerKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      void sendChatMessage();
    }
  }
</script>

<footer class="flex shrink-0 flex-col gap-1.5 border-t border-stone-200 p-2 dark:border-stone-800">
  {#if confirmingClear}
    <div class="flex items-center gap-2 rounded-lg bg-parchment/60 px-2 py-1.5 text-sm dark:bg-stone-800/40">
      <span class="min-w-0 flex-1 text-stone-600 dark:text-stone-300">Clear this conversation?</span>
      <ConfirmButtons
        confirmLabel="Clear"
        onConfirm={() => {
          confirmingClear = false;
          void clearChatThread();
        }}
        onCancel={() => (confirmingClear = false)}
      />
    </div>
  {/if}

  <div class="flex items-center gap-1.5">
    <select
      aria-label="Model"
      value={chat.modelId}
      onchange={(e) => setChatModel((e.currentTarget as HTMLSelectElement).value)}
      class="min-w-0 flex-1 rounded-lg border border-stone-200 bg-parchment px-2 py-0.5 text-caption text-stone-600 dark:border-stone-700 dark:bg-stone-800 dark:text-stone-300"
    >
      {#each chat.models as m (m.id)}
        <option value={m.id}>{m.label}</option>
      {/each}
    </select>
    {#if chat.messages.length > 0 || chat.pending !== null}
      <button
        type="button"
        aria-label="Clear conversation"
        onclick={() => (confirmingClear = true)}
        class="rounded-lg p-1 text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
      >
        <Eraser size={14} />
      </button>
    {/if}
  </div>

  <div class="flex items-end gap-2">
    <textarea
      id={COMPOSER_ID}
      bind:value={chat.draft}
      onkeydown={onComposerKeydown}
      rows="2"
      placeholder="Ask about this paper…"
      class="min-h-0 flex-1 resize-none rounded-lg border border-stone-200 bg-parchment px-2 py-1.5 text-sm outline-none focus:border-amber-700 dark:border-stone-700 dark:bg-stone-800 dark:focus:border-amber-500"
    ></textarea>
    {#if chat.busy}
      <button
        type="button"
        onclick={stopChatStream}
        class="inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-3 py-1.5 text-sm font-medium text-stone-600 hover:bg-parchment dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800"
      >
        <Square size={13} /> Stop
      </button>
    {:else}
      <button
        type="button"
        onclick={() => void sendChatMessage()}
        disabled={!chat.draft.trim()}
        class="inline-flex items-center gap-1.5 rounded-lg bg-amber-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-amber-800 disabled:opacity-50 dark:bg-amber-600 dark:hover:bg-amber-500"
      >
        <SendHorizontal size={14} /> Send
      </button>
    {/if}
  </div>
</footer>
