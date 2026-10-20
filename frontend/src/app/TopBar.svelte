<script lang="ts">
  import { Monitor, Moon, Sun, Upload } from 'lucide-svelte';
  import { openImport } from '$features/importing/queue.svelte';
  import { library, stats } from '$features/library/state.svelte';
  import { filters } from '$features/library/search/state.svelte';
  import { setStatusFilter } from '$features/library/search/actions';
  import { nextTheme, theme, toggleTheme } from '$shared/ui/theme.svelte';
  import { revealLibrary } from '$features/reader/tabs/state.svelte';
  import SealMark from '$shared/ui/SealMark.svelte';
  import SearchBox from '$features/library/search/SearchBox.svelte';

  const THEME_NAMES = { light: 'Light', dark: 'Dark', system: 'System' } as const;
  const themeLabel = $derived(THEME_NAMES[theme.mode]);
  const nextThemeLabel = $derived(THEME_NAMES[nextTheme()]);

  // While searching, library.papers holds relevance-ranked search results
  // rather than the whole library — show how many matched instead of the
  // library-wide total.
  const searching = $derived(filters.q.trim() !== '');
  const matchCount = $derived(library.papers.length);

  function showNeedsReview() {
    revealLibrary();
    void setStatusFilter('needs_review');
  }
</script>

<header class="flex h-14 shrink-0 items-center gap-4 border-b border-stone-200 bg-paper px-4 dark:border-stone-800 dark:bg-night">
  <div class="flex shrink-0 items-center gap-2">
    <SealMark size={22} />
    <span class="hidden font-serif text-lg font-semibold tracking-tight md:inline">Xuewen</span>
  </div>
  <div class="min-w-0 flex-1">
    <div class="mx-auto max-w-xl"><SearchBox /></div>
  </div>
  <div class="flex shrink-0 items-center gap-3">
    {#if stats.value}
      <div class="hidden items-center gap-3 text-xs text-stone-500 sm:flex dark:text-stone-400">
        {#if searching}
          <span>{matchCount} {matchCount === 1 ? 'match' : 'matches'}</span>
        {:else}
          <span>{stats.value.total} papers</span>
        {/if}
        {#if stats.value.needs_review > 0}
          <button
            type="button"
            title="Show papers that need review"
            onclick={showNeedsReview}
            class="rounded text-yellow-700 underline-offset-2 hover:underline dark:text-yellow-400"
          >{stats.value.needs_review} to review</button>
        {/if}
      </div>
    {/if}
    <button
      type="button"
      onclick={openImport}
      class="inline-flex items-center gap-1.5 rounded-lg bg-amber-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-amber-800 dark:bg-amber-600 dark:hover:bg-amber-500"
    >
      <Upload size={16} /> Import
    </button>
    <button
      type="button"
      onclick={toggleTheme}
      aria-label={`Theme: ${themeLabel} — click for ${nextThemeLabel}`}
      title={`Theme: ${themeLabel} — click for ${nextThemeLabel}`}
      class="rounded-lg p-2 text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
    >
      {#if theme.mode === 'light'}<Sun size={18} />{:else if theme.mode === 'dark'}<Moon size={18} />{:else}<Monitor size={18} />{/if}
    </button>
  </div>
</header>
