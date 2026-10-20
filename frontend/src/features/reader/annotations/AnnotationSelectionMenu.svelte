<script lang="ts">
  import { Trash2 } from 'lucide-svelte';
  import type { AnnotationSelectionMenuProps } from '@embedpdf/plugin-annotation/svelte';
  import { deleteSelectedAnnotations } from './commands';
  import { KIND_LABELS, kindOf } from './adapter';
  import { btn } from '$features/reader/pdf/pillStyles';

  // Rendered by <AnnotationLayer>'s selectionMenuSnippet for the one mark the
  // reader has selected — the mouse counterpart of the Delete key, which is why
  // it calls the same command rather than the plugin directly (see
  // annotations/commands.ts).
  let {
    menuWrapperProps,
    context,
  }: Pick<AnnotationSelectionMenuProps, 'menuWrapperProps' | 'context'> = $props();

  // "Delete highlight", not "Delete annotation". A mark that came baked into
  // the PDF from another reader can be a subtype we never store, and guessing a
  // name for it would be worse than the generic one.
  const kind = $derived(kindOf(context.annotation.object));
  const label = $derived(`Delete ${kind ? KIND_LABELS[kind].toLowerCase() : 'annotation'}`);
</script>

<!-- The wrapper's style/action come from the plugin: it positions the box over
     the mark, counter-rotates it against page rotation, and stops pointerdown
     at capture so pressing a control in here can't reach the page underneath
     and deselect the very mark being acted on. Both are passed through
     untouched; only what sits inside is ours. -->
{#if !context.structurallyLocked}
  <div style={menuWrapperProps.style} use:menuWrapperProps.action>
    <!-- pointer-events back on (the wrapper turns them off so the rest of the
         mark's box stays click-through), and centred just above the mark. -->
    <div
      class="pointer-events-auto absolute bottom-full left-1/2 mb-2 flex -translate-x-1/2 items-center gap-1 rounded-xl border border-stone-200 bg-paper/90 px-1.5 py-1 shadow backdrop-blur dark:border-stone-800 dark:bg-soot/90"
    >
      <button
        type="button"
        class={btn}
        aria-label={label}
        title={label}
        onclick={() => deleteSelectedAnnotations()}
      >
        <Trash2 size={16} />
      </button>
    </div>
  </div>
{/if}
