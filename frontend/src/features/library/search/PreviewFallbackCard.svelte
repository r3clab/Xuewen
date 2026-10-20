<script lang="ts">
  import type { PaperSummary } from '$shared/api/generated/PaperSummary';

  // Everything here comes off the row the picker already has, so a paper
  // whose PDF can't be rendered costs no extra request.
  let { paper }: { paper: PaperSummary } = $props();

  const authors = $derived(
    paper.authors.length > 3 ? `${paper.authors.slice(0, 3).join(', ')} et al.` : paper.authors.join(', '),
  );
  const where = $derived([paper.venue, paper.year].filter(Boolean).join(' · '));
</script>

<div
  class="m-auto flex w-full max-w-[250px] flex-col gap-2 rounded-lg border border-dashed border-stone-300 bg-paper p-5 dark:border-stone-700 dark:bg-night"
>
  <h4 class="font-serif text-[15px] font-semibold leading-tight text-ink dark:text-stone-100">
    {paper.title ?? '(untitled)'}
  </h4>
  {#if authors}
    <p class="text-detail text-stone-500 dark:text-stone-400">{authors}</p>
  {/if}
  {#if where}
    <p class="text-detail text-stone-500 dark:text-stone-400">{where}</p>
  {/if}
  <p class="border-t border-stone-200 pt-2 text-caption text-stone-400 dark:border-stone-800 dark:text-stone-500">
    Preview unavailable — the PDF could not be rendered.
  </p>
</div>
