<script lang="ts">
  import { ExternalLink } from '@lucide/svelte';
  import type { PaperDetail } from '$shared/api/generated/PaperDetail';
  import { abbreviateVenue } from '$features/library/venue';
  import StatusPill from '$features/library/StatusPill.svelte';
  import { safeHttpUrl } from '$shared/browser/url';

  let { d }: { d: PaperDetail } = $props();
  const venueLabel = $derived(abbreviateVenue(d.venue));

  type Link = { label: string; href: string };
  const links = $derived.by(() => {
    const out: Link[] = [];
    if (d.doi) out.push({ label: 'DOI', href: `https://doi.org/${d.doi}` });
    if (d.arxiv_id) out.push({ label: 'arXiv', href: `https://arxiv.org/abs/${d.arxiv_id}` });
    if (d.dblp_key) out.push({ label: 'DBLP', href: `https://dblp.org/rec/${d.dblp_key}.html` });
    if (d.url && safeHttpUrl(d.url)) out.push({ label: 'URL', href: d.url });
    return out;
  });
</script>

<h2 class="text-balance font-serif text-xl font-semibold leading-snug text-ink dark:text-stone-100">
  {d.title ?? '(untitled)'}
</h2>
{#if d.authors.length}
  <p class="mt-2 text-[13px] leading-relaxed text-stone-600 dark:text-stone-300">{d.authors.join(', ')}</p>
{/if}
<div class="mt-2.5 flex flex-wrap items-center gap-x-3 gap-y-1.5 text-xs text-stone-500 dark:text-stone-400">
  {#if d.venue || d.year}
    <span>{#if d.venue}<span title={d.venue}>{venueLabel}</span>{/if}{d.venue && d.year ? ' · ' : ''}{d.year ?? ''}</span>
  {/if}
  <StatusPill status={d.status} />
</div>
{#if links.length}
  <div class="mt-3 flex flex-wrap items-center gap-1.5 border-t border-stone-200 pt-3 dark:border-stone-800">
    {#each links as l (l.label)}
      <a
        href={l.href}
        target="_blank"
        rel="noreferrer"
        class="inline-flex items-center gap-1 rounded-full border border-stone-200 px-2 py-0.5 font-mono text-[11px] text-stone-600 hover:border-amber-700 hover:text-amber-700 dark:border-stone-700 dark:text-stone-300 dark:hover:border-amber-500 dark:hover:text-amber-500"
      >
        {l.label}<ExternalLink size={10} />
      </a>
    {/each}
  </div>
{/if}
