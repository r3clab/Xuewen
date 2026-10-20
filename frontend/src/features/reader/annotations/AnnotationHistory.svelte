<script lang="ts">
  import { Redo2, Undo2 } from 'lucide-svelte';
  import { useHistoryCapability } from '@embedpdf/plugin-history/svelte';
  import { annotationHistoryFlags, redoAnnotation, undoAnnotation } from './commands';
  import { btn } from '$features/reader/pdf/pillStyles';

  let { documentId }: { documentId: string } = $props();

  // The scope is read here only to MIRROR the stack's state. Acting goes
  // through the shared commands, so a click and a ⌘Z take one identical path —
  // the same rule the floating trash button follows for delete. Those resolve
  // against the active tab rather than this component's documentId, which is
  // the same document whenever a click can happen at all: an inactive tab is
  // visibility:hidden, so its toolbar is neither clickable nor tabbable.
  const history = useHistoryCapability();
  const scope = $derived(history.provides?.forDocument(documentId) ?? null);

  // canUndo/canRedo are plain method calls on the capability, not reactive
  // state — mirror them into runes off the plugin's own change event so the
  // buttons enable and disable as marks are drawn.
  let canUndo = $state(false);
  let canRedo = $state(false);

  $effect(() => {
    const s = scope;
    if (!s) return;
    const sync = (): void => {
      const flags = annotationHistoryFlags(s);
      canUndo = flags.canUndo;
      canRedo = flags.canRedo;
    };
    sync(); // a tab reopened mid-session may already have a stack
    return s.onHistoryChange(sync);
  });
</script>

<button
  type="button"
  class={btn}
  aria-label="Undo annotation"
  title="Undo annotation"
  disabled={!canUndo}
  onclick={undoAnnotation}
>
  <Undo2 size={16} />
</button>
<button
  type="button"
  class={btn}
  aria-label="Redo annotation"
  title="Redo annotation"
  disabled={!canRedo}
  onclick={redoAnnotation}
>
  <Redo2 size={16} />
</button>
