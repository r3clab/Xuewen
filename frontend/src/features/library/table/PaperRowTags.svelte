<script lang="ts">
  import { Bookmark } from 'lucide-svelte';
  import { filters } from '$features/library/search/state.svelte';
  import { isPrefixMatch } from '$features/library/organization/tagTree';
  import type { PaperSummary } from '$shared/api/generated/PaperSummary';

  // A table row may not wrap, because a row that wraps is two or three times
  // the height of every other row — so the paper with the most tags is the
  // one that looks broken. Collapsed, this is exactly one line: the chips
  // clip at the column edge and the +N stays pinned beside them.
  let { paper }: { paper: PaperSummary } = $props();

  // Project badges never count toward this — only topical tag chips do.
  const CAP = 3;
  let expanded = $state(false);

  function isHit(tagName: string): boolean {
    return filters.tag != null && isPrefixMatch(tagName, filters.tag);
  }

  // A tag chip is visible when expanded, within the first CAP, or (even
  // beyond the cap) it matches the active tag filter — mirrors the approved
  // mock's layoutChips(): `show = expanded || i < CAP || isHit`.
  const collapsedTags = $derived(paper.tags.filter((t, i) => i < CAP || isHit(t.name)));
  const visibleTags = $derived(expanded ? paper.tags : collapsedTags);
  // How many the collapsed view hides — computed from the collapsed set (not
  // `visibleTags`) so the toggle persists once expanded, giving a way to fold
  // back. Was `paper.tags.length - visibleTags.length`, which went to 0 on
  // expand and made the control vanish with no collapse affordance.
  const overflowCount = $derived(paper.tags.length - collapsedTags.length);

  // Clipping is what keeps the row one line high, so the chips carry
  // `shrink-0` — a shrunk chip would render its label torn in half rather
  // than letting the strip cut it off cleanly at the column edge. What
  // clipping costs is discoverability, which the tooltip buys back:
  // it names every badge and chip, including the ones the cut edge hides.
  const allNames = $derived(
    [...paper.projects.map((pr) => pr.name), ...paper.tags.map((t) => t.name)].join(', '),
  );

  const badgeClasses =
    'inline-flex shrink-0 items-center gap-1 rounded border border-indigo-600/30 bg-indigo-600/10 px-1.5 py-0.5 text-[10px] font-semibold text-indigo-800 dark:border-indigo-400/30 dark:bg-indigo-400/10 dark:text-indigo-300';

  function chipClasses(hit: boolean): string {
    return hit
      ? 'shrink-0 rounded border border-amber-700/40 bg-amber-700/10 px-1.5 py-0.5 text-[10px] font-semibold text-amber-800 dark:border-amber-500/40 dark:bg-amber-500/10 dark:text-amber-400'
      : 'shrink-0 rounded border border-stone-200 px-1.5 py-0.5 text-[10px] text-stone-500 dark:border-stone-700 dark:text-stone-400';
  }

  const moreClasses =
    'shrink-0 rounded border border-dashed border-stone-300 px-1.5 py-0.5 text-[10px] font-semibold text-stone-500 hover:border-stone-400 hover:text-stone-700 dark:border-stone-600 dark:text-stone-400 dark:hover:border-stone-500 dark:hover:text-stone-200';

  // Expanding is the one way a row is allowed to grow taller: it is asked
  // for, it is labelled, and "Less" puts it back.
  const outerClasses = $derived(`flex min-w-0 items-center gap-1${expanded ? ' flex-wrap' : ''}`);
  // Collapsed, the chips get their own shrinkable, clipping strip so the +N —
  // a sibling of the strip, not a member — keeps its place at the right
  // however narrow the Tags column gets. Expanded, the strip is
  // `display: contents`, leaving the chips direct children of the wrapping
  // flex container.
  const stripClasses = $derived(
    expanded ? 'contents' : 'flex min-w-0 items-center gap-1 overflow-hidden',
  );

  // Rows are themselves a clickable container (opens the paper) — stop the
  // +N control's click from bubbling up and opening it too.
  function onMoreClick(e: MouseEvent) {
    e.stopPropagation();
    expanded = !expanded;
  }
</script>

{#if paper.projects.length || paper.tags.length}
  <div class={outerClasses}>
    <div class={stripClasses} title={expanded ? undefined : allNames}>
      {#each paper.projects as project (project.id)}
        <span class={badgeClasses}>
          <Bookmark size={9} />
          {project.name}
        </span>
      {/each}
      {#each visibleTags as tag (tag.id)}
        <span class={chipClasses(isHit(tag.name))}>{tag.name}</span>
      {/each}
    </div>
    {#if overflowCount > 0}
      <button type="button" onclick={onMoreClick} aria-expanded={expanded} class={moreClasses}>
        {expanded ? 'Less' : `+${overflowCount}`}
      </button>
    {/if}
  </div>
{/if}
