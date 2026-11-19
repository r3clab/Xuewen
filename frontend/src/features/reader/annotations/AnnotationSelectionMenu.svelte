<script lang="ts">
  import { Trash } from '@lucide/svelte';
  import { useAnnotation, useAnnotationState } from '@embedpdf/svelte/annotation';
  import { deleteSelectedAnnotations } from './commands';
  import { KIND_LABELS, kindOf } from './adapter';
  import { btn } from '$features/reader/pdf/pillStyles';

  // Rendered by <AnnotationMenu> next to the mark the reader has selected —
  // the mouse counterpart of the Delete key, which is why it calls the same
  // command rather than the plugin directly (see annotations/commands.ts).
  const annotation = useAnnotation();
  const state = useAnnotationState();

  // Never for a multi-selection: one trash button for several marks reads as
  // deleting the one it sits next to.
  const selected = $derived(state.selected.length === 1 ? state.selected[0] : null);
  // "Delete highlight", not "Delete annotation". A mark that came baked into
  // the PDF from another reader can be a subtype we never store, and guessing a
  // name for it would be worse than the generic one.
  const kind = $derived(selected ? kindOf(selected) : null);
  const label = $derived(`Delete ${kind ? KIND_LABELS[kind].toLowerCase() : 'annotation'}`);
</script>

{#if selected && annotation.canDelete(selected.ref)}
  <div
    class="flex items-center gap-1 rounded-xl border border-stone-200 bg-paper/90 px-1.5 py-1 shadow backdrop-blur dark:border-stone-800 dark:bg-soot/90"
  >
    <button
      type="button"
      class={btn}
      aria-label={label}
      title={label}
      onclick={() => deleteSelectedAnnotations()}
    >
      <Trash size={16} />
    </button>
  </div>
{/if}
