<script lang="ts">
  import { Trash2 } from 'lucide-svelte';
  import ConfirmButtons from '$shared/ui/ConfirmButtons.svelte';
  import { removePaper } from '$app/actions';

  let { id }: { id: string } = $props();

  let confirming = $state(false);
  let deleting = $state(false);

  async function doDelete() {
    deleting = true;
    // removePaper never rejects — success and failure both surface as its
    // own toasts. On success the surrounding panel unmounts (its tab
    // closes); on failure it doesn't, so Deleting… must reset either way.
    await removePaper(id);
    deleting = false;
  }
</script>

{#if confirming}
  {#if deleting}
    <span class="block text-sm text-stone-500 dark:text-stone-400">Deleting…</span>
  {:else}
    <p class="text-xs text-stone-600 dark:text-stone-300">Delete this paper?</p>
    <div class="mt-1.5 flex gap-2">
      <ConfirmButtons
        confirmLabel="Delete"
        onConfirm={doDelete}
        onCancel={() => (confirming = false)}
      />
    </div>
  {/if}
{:else}
  <button
    type="button"
    onclick={() => (confirming = true)}
    class="inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1 text-xs font-medium text-red-600 hover:bg-red-600/10 dark:border-stone-700 dark:text-red-400"
  >
    <Trash2 size={13} /> Delete paper
  </button>
{/if}
