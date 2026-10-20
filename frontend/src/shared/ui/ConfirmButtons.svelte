<script lang="ts">
  // The shared destructive-action button pair (red confirm + neutral cancel).
  // Owns only the two buttons; the caller owns the confirming state, the
  // prompt text, and any surrounding layout.
  let {
    confirmLabel,
    onConfirm,
    onCancel,
  }: {
    confirmLabel: string;
    onConfirm: () => void;
    onCancel: () => void;
  } = $props();
</script>

<button
  type="button"
  onclick={onConfirm}
  class="rounded-lg bg-red-600 px-2 py-1 text-xs font-medium text-white hover:bg-red-700"
>
  {confirmLabel}
</button>
<button
  type="button"
  onclick={onCancel}
  class="rounded-lg px-2 py-1 text-xs text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
>
  Cancel
</button>
