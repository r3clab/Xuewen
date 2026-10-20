<!--
  Rename/delete menu for one pill (project or tag): a generic two-step
  machine keyed by {kind, name}, on the shared ContextMenuShell. Escape steps
  back one level instead of closing outright, and scroll dismissal is off —
  the filter strip sits above the scrolling table, so a table scroll never
  moves the anchor and auto-closing would only discard an in-progress rename.
-->
<script lang="ts">
  import ConfirmButtons from '$shared/ui/ConfirmButtons.svelte';
  import ContextMenuShell from '$shared/ui/ContextMenuShell.svelte';

  let {
    kind,
    name,
    x,
    y,
    onRename,
    onDelete,
    onClose,
  }: {
    kind: 'project' | 'tag';
    name: string;
    x: number;
    y: number;
    onRename: (name: string) => Promise<void>;
    onDelete: () => Promise<void>;
    onClose: () => void;
  } = $props();

  let mode = $state<'menu' | 'rename' | 'delete'>('menu');
  let renameValue = $state('');
  let renameInput = $state<HTMLInputElement | null>(null);
  let confirmEl = $state<HTMLDivElement | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);

  // Rename focuses its input; the delete confirm focuses its first button so
  // Enter-ing "Delete" flows straight into confirm-or-cancel by keyboard.
  $effect(() => {
    if (mode === 'rename') renameInput?.focus();
    else if (mode === 'delete') confirmEl?.querySelector<HTMLElement>('button')?.focus();
  });

  function backToMenu() {
    mode = 'menu';
    error = null;
  }

  // Escape steps back one level (delete-confirm/rename → action list →
  // closed). The rename input handles its own Escape and stops propagation,
  // so the shell's listener only sees it from the action list and the
  // delete confirm.
  function onEscape() {
    if (mode === 'menu') onClose();
    else backToMenu();
  }

  function startRename() {
    renameValue = name;
    mode = 'rename';
    error = null;
  }
  async function submitRename() {
    const next = renameValue.trim();
    if (!next || next === name) {
      onClose();
      return;
    }
    busy = true;
    error = null;
    try {
      await onRename(next);
      onClose();
    } catch (e) {
      error = (e as Error).message;
    } finally {
      busy = false;
    }
  }
  function onRenameKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      void submitRename();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      backToMenu();
    }
  }

  function startDelete() {
    mode = 'delete';
    error = null;
  }
  async function confirmDelete() {
    busy = true;
    error = null;
    try {
      await onDelete();
      onClose();
    } catch (e) {
      error = (e as Error).message;
    } finally {
      busy = false;
    }
  }
</script>

<ContextMenuShell
  {x}
  {y}
  label={`${name} options`}
  width="w-36"
  dismissOnScroll={false}
  {onClose}
  {onEscape}
>
  {#if mode === 'menu'}
    <button
      type="button"
      role="menuitem"
      onclick={startRename}
      class="block w-full rounded-lg px-2 py-1 text-left text-xs text-stone-600 hover:bg-parchment hover:text-ink dark:text-stone-300 dark:hover:bg-stone-800"
    >
      Rename
    </button>
    <button
      type="button"
      role="menuitem"
      onclick={startDelete}
      class="block w-full rounded-lg px-2 py-1 text-left text-xs text-red-600 hover:bg-red-600/10 dark:text-red-400"
    >
      Delete
    </button>
  {:else if mode === 'rename'}
    <input
      bind:this={renameInput}
      bind:value={renameValue}
      type="text"
      aria-label={`Rename ${name}`}
      onkeydown={onRenameKeydown}
      class="w-full rounded-lg border border-stone-200 bg-paper px-1.5 py-1 text-xs outline-none focus:border-indigo-600 dark:border-stone-700 dark:bg-stone-800"
    />
    <div class="mt-1 flex justify-end gap-1">
      <button
        type="button"
        onclick={backToMenu}
        class="rounded-lg px-2 py-0.5 text-xs text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
      >
        Cancel
      </button>
      <button
        type="button"
        disabled={busy}
        onclick={() => void submitRename()}
        class="rounded-lg bg-indigo-600 px-2 py-0.5 text-xs font-medium text-white hover:bg-indigo-700 disabled:opacity-50 dark:bg-indigo-500"
      >
        Save
      </button>
    </div>
  {:else if busy}
    <span class="block px-1 py-0.5 text-xs text-stone-500 dark:text-stone-400">Deleting…</span>
  {:else}
    <p class="px-1 text-xs text-stone-600 dark:text-stone-300">Delete this {kind}?</p>
    <div bind:this={confirmEl} class="mt-1 flex justify-end gap-1">
      <ConfirmButtons
        confirmLabel="Delete"
        onConfirm={() => void confirmDelete()}
        onCancel={backToMenu}
      />
    </div>
  {/if}
  {#if error}
    <p class="mt-1 px-1 text-chip text-red-600 dark:text-red-400">{error}</p>
  {/if}
</ContextMenuShell>
