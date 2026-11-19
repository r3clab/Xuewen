<script lang="ts">
  import { focusOnMount } from './focus';

  // The shared destructive-action button pair (red confirm + neutral cancel).
  // Owns only the two buttons; the caller owns the confirming state, the
  // prompt text, and any surrounding layout.
  let {
    confirmLabel,
    onConfirm,
    onCancel,
    autofocus = false,
  }: {
    confirmLabel: string;
    onConfirm: () => void;
    onCancel: () => void;
    /// Focus the confirm button as the pair appears, so a keyboard user who
    /// chose the destructive action lands on confirm-or-cancel.
    autofocus?: boolean;
  } = $props();
</script>

<button
  type="button"
  {@attach autofocus && focusOnMount}
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
