<script lang="ts">
  import { ChevronDown, Highlighter, MessageSquare, Strikethrough, Underline, Waves } from 'lucide-svelte';
  import { useAnnotation } from '@embedpdf/plugin-annotation/svelte';
  import { PdfAnnotationSubtype } from '@embedpdf/models';
  import ToolbarMenu from '$features/reader/pdf/ToolbarMenu.svelte';
  import {
    colorPatch,
    KIND_LABELS,
    kindOf,
    TOOL_BY_KIND,
  } from './adapter';
  import { annotationTools, setToolColor, toggleTool } from './state.svelte';
  import {
    ANNOTATION_COLORS,
    colorHex,
    colorLabel,
  } from './palette';
  import { activeBtn, btn } from '$features/reader/pdf/pillStyles';
  import type { AnnotationColor } from '$shared/api/generated/AnnotationColor';
  import type { AnnotationKind } from '$shared/api/generated/AnnotationKind';

  let { documentId, onHoldChange }: { documentId: string; onHoldChange: (held: boolean) => void } =
    $props();

  const annotation = useAnnotation(() => documentId);

  let menuOpen = $state(false);
  $effect(() => onHoldChange(menuOpen));

  // Only the icon is toolbar-specific; the names come from the adapter, which
  // the sidebar list reads too.
  const ICONS: Record<AnnotationKind, typeof Highlighter> = {
    highlight: Highlighter,
    underline: Underline,
    strikeout: Strikethrough,
    squiggly: Waves,
    text_comment: MessageSquare,
  };
  const TOOLS = (Object.keys(ICONS) as AnnotationKind[]).map((kind) => ({
    kind,
    label: KIND_LABELS[kind],
    icon: ICONS[kind],
  }));

  const active = $derived(annotationTools.active);
  const activeTool = $derived(TOOLS.find((t) => t.kind === active));
  const TriggerIcon = $derived(activeTool?.icon ?? Highlighter);

  // The armed tool lives in app state (global, so it survives a tab switch);
  // the plugin needs telling per document. Driving it from an effect rather
  // than from the click handler means a newly opened tab inherits whatever is
  // armed, instead of looking disarmed until the next click.
  $effect(() => {
    const scope = annotation.provides;
    if (!scope) return;
    scope.setActiveTool(active === null ? null : TOOL_BY_KIND[active]);
  });

  // The color → tool-defaults push is NOT here: tool defaults are global in
  // the plugin (registry-wide, not per-document), and this component mounts
  // once per open tab — PdfDeck, of which there is one live instance, owns it.

  /// Picking a color both arms the next mark and recolors whatever is selected
  /// — the selected-mark case is what a reader expects after drawing one and
  /// deciding it should have been a different color.
  function pickColor(c: AnnotationColor): void {
    setToolColor(c);
    const scope = annotation.provides;
    if (!scope) return;
    const hex = colorHex(c);
    for (const sel of scope.getSelectedAnnotations()) {
      // A mark whose subtype isn't one of ours (a foreign one the reader
      // happens to have selected) is left alone rather than guessed at.
      const kind = kindOf(sel.object);
      if (kind) scope.updateAnnotation(sel.object.pageIndex, sel.object.id, colorPatch(kind, hex));
    }
  }
</script>

<ToolbarMenu bind:open={menuOpen} label="Annotation tools" panelClass="w-max p-1.5">
  {#snippet trigger()}
    <button
      type="button"
      class={`${active ? activeBtn : btn} flex items-center gap-0.5`}
      aria-label={active ? `Annotation tool: ${activeTool?.label}` : 'Annotation tools'}
      aria-expanded={menuOpen}
      title={active ? `${activeTool?.label} — click to change` : 'Annotate'}
      onclick={() => (menuOpen = !menuOpen)}
    >
      <TriggerIcon size={16} />
      <ChevronDown size={12} />
    </button>
  {/snippet}
  <div class="flex items-center gap-0.5">
    {#each TOOLS as tool (tool.kind)}
      <button
        type="button"
        role="menuitemradio"
        aria-checked={active === tool.kind}
        class={active === tool.kind ? activeBtn : btn}
        aria-label={tool.label}
        title={tool.label}
        onclick={() => toggleTool(tool.kind)}
      >
        <tool.icon size={16} />
      </button>
    {/each}
  </div>
  <div class="mt-1.5 flex items-center gap-1 border-t border-stone-200 pt-1.5 dark:border-stone-800">
    {#each ANNOTATION_COLORS as c (c)}
      <button
        type="button"
        role="menuitemradio"
        aria-checked={annotationTools.color === c}
        class={`h-5 w-5 rounded-full border-2 ${
          annotationTools.color === c
            ? 'border-ink dark:border-stone-100'
            : 'border-transparent hover:border-stone-300 dark:hover:border-stone-600'
        }`}
        style:background-color={colorHex(c)}
        aria-label={colorLabel(c)}
        title={colorLabel(c)}
        onclick={() => pickColor(c)}
      ></button>
    {/each}
  </div>
</ToolbarMenu>
