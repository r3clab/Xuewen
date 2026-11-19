<script lang="ts">
  import { usePageList } from '@embedpdf/svelte/runtime';
  import { useStage } from '@embedpdf/svelte/stage';
  import type { CitationData } from './references';
  import type { PaperSummary } from '$shared/api/generated/PaperSummary';
  import { showCitation, hideCitationSoon } from './state.svelte';

  let {
    pageIndex,
    pageWidthPt,
    pageHeightPt,
    data,
    matches,
  }: {
    pageIndex: number;
    pageWidthPt: number;
    pageHeightPt: number;
    data: CitationData;
    matches: Map<number, PaperSummary>;
  } = $props();

  const stage = useStage();
  const pages = usePageList();

  const markers = $derived(data.markers.filter((m) => m.pageIndex === pageIndex));

  function enter(e: PointerEvent, refIndex: number) {
    const reference = data.references[refIndex];
    if (!reference) return;
    showCitation({
      reference,
      matchedPaper: matches.get(refIndex) ?? null,
      screenX: e.clientX,
      screenY: e.clientY,
    });
  }

  // Hover previews; click (or Enter) jumps to the entry in the bibliography.
  // destY is top-left page space; back off a line so the entry isn't flush
  // with the viewport edge.
  function jump(refIndex: number) {
    const reference = data.references[refIndex];
    const page = reference ? pages.current[reference.destPageIndex] : undefined;
    if (!reference || !page) return;
    stage.goToDestination(
      { kind: 'xyz', page: page.ref, x: null, y: Math.max(0, reference.destY - 24), zoom: null },
      { behavior: 'smooth' },
    );
  }
</script>

<!-- Absolutely-positioned, non-blocking layer filling the page box. Each hit-box
     is a % of the page so it tracks zoom automatically. pointer-events only on
     the boxes so text selection underneath still works. -->
<div class="pointer-events-none absolute inset-0">
  {#each markers as m (`${m.x},${m.y},${m.refIndex}`)}
    <span
      role="link"
      tabindex="0"
      aria-label="Citation reference {m.refIndex + 1} — click to jump to the bibliography"
      onpointerenter={(e) => enter(e, m.refIndex)}
      onpointerleave={hideCitationSoon}
      onclick={() => jump(m.refIndex)}
      onkeydown={(e) => e.key === 'Enter' && jump(m.refIndex)}
      style:left="{(m.x / pageWidthPt) * 100}%"
      style:top="{(m.y / pageHeightPt) * 100}%"
      style:width="{(m.width / pageWidthPt) * 100}%"
      style:height="{(m.height / pageHeightPt) * 100}%"
      class="pointer-events-auto absolute cursor-pointer rounded-sm border-b-2 border-dotted border-amber-600/55 bg-amber-400/0 hover:border-amber-600/0 hover:bg-amber-400/25 dark:border-amber-500/60"
    ></span>
  {/each}
</div>
