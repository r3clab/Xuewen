<script lang="ts">
  let { status, class: cls = '' }: { status: string; class?: string } = $props();
  // "resolved" is the normal state, so badging it everywhere is just noise.
  // Only flag the exception — a paper that still needs review — so unresolved
  // papers stay easy to spot in the list and the info panel.
  const needsReview = $derived(status !== 'resolved');
</script>

{#if needsReview}
  <span
    class={`inline-flex items-center rounded-full bg-yellow-100 px-2 py-0.5 text-xs font-medium text-yellow-800 dark:bg-yellow-500/15 dark:text-yellow-300 ${cls}`}
  >
    Needs review
  </span>
{/if}
