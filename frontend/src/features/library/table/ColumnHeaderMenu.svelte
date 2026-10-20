<script lang="ts">
  import { MoveHorizontal, RotateCcw } from 'lucide-svelte';
  import ContextMenuShell from '$shared/ui/ContextMenuShell.svelte';

  let {
    x,
    y,
    onAutoFitAll,
    onReset,
    onClose,
  }: {
    x: number;
    y: number;
    onAutoFitAll: () => void;
    onReset: () => void;
    onClose: () => void;
  } = $props();

  const itemClasses =
    'flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-xs text-stone-600 hover:bg-parchment hover:text-ink dark:text-stone-300 dark:hover:bg-stone-800';
</script>

<ContextMenuShell {x} {y} label="Column options" width="w-48" {onClose}>
  <button
    type="button"
    role="menuitem"
    class={itemClasses}
    onclick={() => {
      onAutoFitAll();
      onClose();
    }}
  >
    <MoveHorizontal size={13} /> Auto-fit all columns
  </button>
  <button
    type="button"
    role="menuitem"
    class={itemClasses}
    onclick={() => {
      onReset();
      onClose();
    }}
  >
    <RotateCcw size={13} /> Reset to default widths
  </button>
</ContextMenuShell>
