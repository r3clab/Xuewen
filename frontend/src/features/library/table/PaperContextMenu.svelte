<script lang="ts">
  import { Copy, ScanSearch, Trash2 } from 'lucide-svelte';
  import ConfirmButtons from '$shared/ui/ConfirmButtons.svelte';
  import ContextMenuShell from '$shared/ui/ContextMenuShell.svelte';
  import { closeContextMenu, contextMenu } from './contextMenu.svelte';
  import { copyCitation } from '$features/library/cite';
  import { openIdentify } from '$features/identify/actions';
  import { removePaper } from '$app/actions';
  import { toast } from '$shared/ui/toasts.svelte';

  // Two-step delete lives inside the menu (mirrors DeletePaperButton /
  // the pill bar's PillMenu) so a right-click delete still needs a confirm.
  let mode = $state<'menu' | 'delete'>('menu');
  let busy = $state(false);
  let confirmEl = $state<HTMLDivElement | null>(null);

  // Every fresh open starts on the action list, never mid-delete-confirm —
  // including a retarget (right-click on a second row) that swaps the paper
  // without the always-mounted menu ever closing in between.
  $effect(() => {
    void contextMenu.paper; // re-run when the target paper changes
    if (contextMenu.open) {
      mode = 'menu';
      busy = false;
    }
  });

  // Switching to the delete confirm moves focus onto its first button, so
  // Enter-ing "Delete…" flows straight into confirm-or-cancel by keyboard.
  $effect(() => {
    if (mode === 'delete') confirmEl?.querySelector<HTMLElement>('button')?.focus();
  });

  async function doCopy() {
    const paper = contextMenu.paper;
    closeContextMenu();
    if (!paper) return;
    try {
      await copyCitation(paper.id, 'bibtex');
      toast('success', 'BibTeX copied');
    } catch {
      toast('error', "Couldn't copy BibTeX");
    }
  }

  function doIdentify() {
    const paper = contextMenu.paper;
    closeContextMenu();
    if (!paper) return;
    openIdentify(paper.id, { doi: paper.doi, arxiv_id: paper.arxiv_id });
  }

  async function doDelete() {
    const paper = contextMenu.paper;
    if (!paper) return;
    busy = true;
    // removePaper never rejects — the Deleted/Undo toast (or the failure
    // toast) is its own; the menu just closes either way.
    await removePaper(paper.id);
    closeContextMenu();
    busy = false;
  }

  const itemClasses =
    'flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-xs text-stone-600 hover:bg-parchment hover:text-ink dark:text-stone-300 dark:hover:bg-stone-800';
</script>

{#if contextMenu.open && contextMenu.paper}
  <ContextMenuShell
    x={contextMenu.x}
    y={contextMenu.y}
    label="Paper actions"
    width="w-44"
    onClose={closeContextMenu}
  >
    {#if mode === 'menu'}
      <button type="button" role="menuitem" onclick={() => void doCopy()} class={itemClasses}>
        <Copy size={13} /> Copy BibTeX
      </button>
      <button type="button" role="menuitem" onclick={doIdentify} class={itemClasses}>
        <ScanSearch size={13} /> Identify…
      </button>
      <div class="my-1 border-t border-stone-200 dark:border-stone-800"></div>
      <button
        type="button"
        role="menuitem"
        onclick={() => (mode = 'delete')}
        class="flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-xs text-red-600 hover:bg-red-600/10 dark:text-red-400"
      >
        <Trash2 size={13} /> Delete…
      </button>
    {:else if busy}
      <span class="block px-2 py-1.5 text-xs text-stone-500 dark:text-stone-400">Deleting…</span>
    {:else}
      <p class="px-1 py-0.5 text-xs text-stone-600 dark:text-stone-300">Delete this paper?</p>
      <div bind:this={confirmEl} class="mt-1 flex justify-end gap-1">
        <ConfirmButtons
          confirmLabel="Delete"
          onConfirm={() => void doDelete()}
          onCancel={() => (mode = 'menu')}
        />
      </div>
    {/if}
  </ContextMenuShell>
{/if}
