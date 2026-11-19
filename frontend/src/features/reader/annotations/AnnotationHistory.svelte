<script lang="ts">
  import { Redo2, Undo2 } from '@lucide/svelte';
  import { useHistoryState } from '@embedpdf/svelte/history';
  import { redoAnnotation, undoAnnotation } from './commands';
  import { btn } from '$features/reader/pdf/pillStyles';

  // The state is read here only to MIRROR the stack. Acting goes through the
  // shared commands, so a click and a ⌘Z take one identical path — the same
  // rule the floating trash button follows for delete. Those resolve against
  // the active tab rather than this toolbar's document, which is the same
  // document whenever a click can happen at all: an inactive tab is
  // visibility:hidden, so its toolbar is neither clickable nor tabbable.
  const history = useHistoryState();
</script>

<button
  type="button"
  class={btn}
  aria-label="Undo annotation"
  title="Undo annotation"
  disabled={!history.canUndo}
  onclick={undoAnnotation}
>
  <Undo2 size={16} />
</button>
<button
  type="button"
  class={btn}
  aria-label="Redo annotation"
  title="Redo annotation"
  disabled={!history.canRedo}
  onclick={redoAnnotation}
>
  <Redo2 size={16} />
</button>
