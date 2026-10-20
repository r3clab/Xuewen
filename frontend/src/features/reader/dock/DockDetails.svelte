<script lang="ts">
  import { Star, Wand2 } from 'lucide-svelte';
  import { openIdentify } from '$features/identify/actions';
  import { cachedDetail } from '$features/library/state.svelte';
  import { loadDetail, toggleStar } from '$features/library/actions';
  import { appSettings } from '$shared/api/settings.svelte';
  import CiteActions from './CiteActions.svelte';
  import DeletePaperButton from './DeletePaperButton.svelte';
  import DockCode from '$features/reader/ask/DockCode.svelte';
  import PaperMeta from './PaperMeta.svelte';
  import PaperNameEditor from '$features/library/organization/PaperNameEditor.svelte';
  import PaperProjectEditor from '$features/library/organization/PaperProjectEditor.svelte';
  import PaperTagEditor from '$features/library/organization/PaperTagEditor.svelte';
  import Spinner from '$shared/ui/Spinner.svelte';

  let { id }: { id: string } = $props();

  let abstractOpen = $state(!appSettings.foldAbstract);

  // The reactive cache read plus fetch-on-miss. A mutation patches the cached
  // record and this view updates in place — no remount, so child editor state
  // (a half-typed tag query, a focused input) survives unrelated mutations —
  // while a global eviction (tag/project rename) empties the entry, re-runs
  // this effect, and refetches.
  const d = $derived(cachedDetail(id));
  let loadFailed = $state(false);
  $effect(() => {
    if (cachedDetail(id) !== undefined) return;
    const requested = id;
    loadFailed = false;
    loadDetail(requested).catch(() => {
      if (requested === id) loadFailed = true;
    });
  });

  function fmtDate(s: string): string {
    if (!s) return '—';
    const dt = new Date(s);
    return isNaN(dt.getTime())
      ? s
      : dt.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
  }

  const label = 'text-caption font-semibold uppercase tracking-[.08em] text-stone-500 dark:text-stone-400';
  const divider = 'mt-4 border-t border-stone-200 pt-4 dark:border-stone-800';
</script>

<!-- Keyed on the paper identity (only): switching papers must still reset the
     children's editor state (PaperNameEditor's draft, the tag query). The
     dock owns the scroll — the record and the Ask thread share it. -->
{#key id}
  {#if d}
    <PaperMeta {d} />

    {#if d.summary}
      <section class={divider}>
        <h3 class={label}>Summary</h3>
        <p class="mt-2 font-serif text-reading font-medium leading-relaxed text-ink dark:text-stone-100">
          {d.summary.tldr}
        </p>
        <dl class="mt-2 space-y-1.5 text-detail">
          {#each [['Problem', d.summary.problem], ['Approach', d.summary.approach], ['Results', d.summary.results], ['Limitations', d.summary.limitations]] as [k, v]}
            <div>
              <dt class="text-caption font-semibold uppercase tracking-[.08em] text-stone-500 dark:text-stone-400">{k}</dt>
              <dd class="text-stone-700 dark:text-stone-300">{v}</dd>
            </div>
          {/each}
        </dl>
      </section>
    {/if}

    {#if d.abstract}
      <section class={divider}>
        <button type="button" onclick={() => (abstractOpen = !abstractOpen)} aria-expanded={abstractOpen} class={`flex items-center gap-1.5 ${label}`}>
          Abstract
          <svg
            class={`h-3 w-3 transition-transform ${abstractOpen ? '' : '-rotate-90'}`}
            viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2"
            stroke-linecap="round" stroke-linejoin="round"
          ><path d="m6 9 6 6 6-6" /></svg>
        </button>
        {#if abstractOpen}
          <p class="mt-2 max-w-[42ch] font-serif text-reading leading-relaxed text-stone-700 dark:text-stone-300">
            {d.abstract}
          </p>
        {/if}
      </section>
    {/if}

    <section class={divider}>
      <h3 class={label}>Record</h3>
      <dl class="mt-2 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-detail">
        <PaperNameEditor {d} />
        {#if d.cite_key}
          <dt class="text-stone-500 dark:text-stone-400">Cite key</dt>
          <dd class="font-mono text-caption text-ink dark:text-stone-200">{d.cite_key}</dd>
        {/if}
        {#if d.source}
          <dt class="text-stone-500 dark:text-stone-400">Source</dt>
          <dd class="text-ink dark:text-stone-200">{d.source}</dd>
        {/if}
        <dt class="text-stone-500 dark:text-stone-400">Added</dt>
        <dd class="text-ink dark:text-stone-200">{fmtDate(d.added_at)}</dd>
      </dl>
    </section>

    <DockCode id={d.id} />

    <section class={divider}>
      <h3 class={label}>Organize</h3>
      <button
        type="button"
        aria-pressed={d.starred}
        onclick={() => void toggleStar(d.id)}
        class={`mt-2 inline-flex items-center gap-1.5 rounded-lg border px-2 py-1 text-xs font-medium ${
          d.starred
            ? 'border-orange-600/50 bg-orange-600/15 text-orange-700 dark:border-orange-400/50 dark:bg-orange-400/15 dark:text-orange-400'
            : 'border-stone-200 text-stone-500 hover:border-orange-600/35 hover:text-orange-700 dark:border-stone-700 dark:text-stone-400'
        }`}
      >
        <Star size={13} fill={d.starred ? 'currentColor' : 'none'} />
        {d.starred ? 'Starred' : 'Star'}
      </button>
      <div class="mt-3">
        <PaperTagEditor {d} />
      </div>
      <div class="mt-4">
        <PaperProjectEditor {d} />
      </div>
    </section>

    <div class={`flex flex-wrap items-center gap-2 ${divider}`}>
      <CiteActions id={d.id} citeKey={d.cite_key} />
      <button
        type="button"
        onclick={() => openIdentify(d.id, { doi: d.doi, arxiv_id: d.arxiv_id })}
        class="inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1 text-xs font-medium text-amber-700 hover:bg-amber-700/10 dark:border-stone-700 dark:text-amber-500"
      >
        <Wand2 size={13} /> Identify…
      </button>
    </div>

    <!-- Its own section rather than the row above: nothing destructive
         shares a row with Copy BibTeX. -->
    <div class={divider}>
      <DeletePaperButton {id} />
    </div>
  {:else if loadFailed}
    <p class="text-sm text-red-600 dark:text-red-400">
      Failed to load details. Check that the server is running, then reopen this panel.
    </p>
  {:else}
    <Spinner />
  {/if}
{/key}
