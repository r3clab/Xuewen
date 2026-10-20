<script lang="ts" module>
  export type Zone = 'projects' | 'tags';
</script>

<script lang="ts">
  import { Bookmark, Ellipsis, Star } from 'lucide-svelte';
  import NewProjectInput from '$features/library/organization/NewProjectInput.svelte';
  import PillMenu from './PillMenu.svelte';
  import {
    createNewProject,
    deleteTag,
    removeProject,
    renameProject,
    renameTag,
  } from '$features/library/organization/actions';
  import { projects, tags } from '$features/library/organization/state.svelte';
  import { filters } from './state.svelte';
  import { setProjectFilter, setStarFilter, setTagFilter } from './actions';

  let { zone }: { zone: Zone } = $props();

  // Shared pill shape; the three kinds differ only in their color pairs.
  const pillBase = 'inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs font-medium';
  function projectPillClasses(active: boolean): string {
    return `${pillBase} ${
      active
        ? 'border-indigo-600 bg-indigo-600 text-white dark:border-indigo-500 dark:bg-indigo-500'
        : 'border-indigo-600/25 bg-indigo-600/10 text-indigo-800 hover:border-indigo-600/45 dark:border-indigo-400/25 dark:bg-indigo-400/10 dark:text-indigo-300'
    }`;
  }
  function starPillClasses(active: boolean): string {
    return `${pillBase} ${
      active
        ? 'border-orange-600/50 bg-orange-600/15 text-orange-700 dark:border-orange-400/50 dark:bg-orange-400/15 dark:text-orange-400'
        : 'border-stone-200 text-orange-700/70 hover:border-orange-600/35 dark:border-stone-700 dark:text-orange-400/70'
    }`;
  }
  function tagPillClasses(active: boolean): string {
    return `${pillBase} ${
      active
        ? 'border-amber-700/40 bg-amber-700/10 text-amber-800 dark:border-amber-500/40 dark:bg-amber-500/10 dark:text-amber-400'
        : 'border-stone-200 text-stone-500 hover:border-stone-300 dark:border-stone-700 dark:text-stone-400'
    }`;
  }

  function onProjectPill(id: string) {
    void setProjectFilter(filters.project === id ? 'all' : id);
  }
  function onStarPill() {
    void setStarFilter(filters.starred !== true);
  }
  function onTagPill(name: string) {
    void setTagFilter(filters.tag === name ? undefined : name);
  }

  // --- per-pill context menu (rename / delete) ---
  // Opens on right-click on the pill itself, mirroring the paper rows'
  // PaperContextMenu: one cursor-anchored, viewport-clamped instance.
  // (Task 14's hover "⋯" sibling reserved layout space next to every pill —
  // opacity-0 keeps the box — which read as `pill … pill …` gaps.) macOS has
  // no Menu key and Shift+F10 doesn't reliably fire contextmenu, so each
  // pill also keeps an "⋯" trigger that is sr-only until keyboard-focused —
  // mouse users never see it and the bar stays gap-free.
  type PillKind = 'project' | 'tag';
  let openMenu = $state<{ kind: PillKind; id: string; name: string } | null>(null);
  let menuX = $state(0);
  let menuY = $state(0);

  function openPillMenu(e: MouseEvent, kind: PillKind, id: string, name: string) {
    e.preventDefault();
    // Keyboard invocation reports (0,0) in some browsers — anchor to the
    // pill itself then.
    const pill = e.currentTarget instanceof HTMLElement ? e.currentTarget : null;
    if (e.clientX === 0 && e.clientY === 0 && pill) {
      const r = pill.getBoundingClientRect();
      menuX = r.left;
      menuY = r.bottom;
    } else {
      menuX = e.clientX;
      menuY = e.clientY;
    }
    openMenu = { kind, id, name };
  }
  function closeMenu() {
    openMenu = null;
  }
  function isMenuOpen(kind: PillKind, id: string): boolean {
    return openMenu?.kind === kind && openMenu.id === id;
  }

  async function renamePill(name: string) {
    if (!openMenu) return;
    if (openMenu.kind === 'project') await renameProject(openMenu.id, { name });
    else await renameTag(openMenu.id, name);
  }
  async function deletePill() {
    if (!openMenu) return;
    if (openMenu.kind === 'project') await removeProject(openMenu.id);
    else await deleteTag(openMenu.id);
  }

  // A keyboard Enter/Space "click" has no coordinates, so openPillMenu's
  // (0,0) fallback anchors the menu to this trigger's own rect.
  const kbdTriggerClasses =
    'sr-only rounded-full text-stone-400 focus-visible:not-sr-only dark:text-stone-500';
</script>

<!-- One pill + sr-only "⋯" trigger pair; project and tag pills differ only
     in palette, icon, and toggle handler. (Starred stays hand-written — it
     has no menu.) -->
{#snippet pill(p: {
  kind: PillKind;
  id: string;
  name: string;
  count: number;
  active: boolean;
  palette: (active: boolean) => string;
  icon?: typeof Bookmark;
  onToggle: () => void;
})}
  <button
    type="button"
    aria-pressed={p.active}
    onclick={p.onToggle}
    oncontextmenu={(e) => openPillMenu(e, p.kind, p.id, p.name)}
    class={p.palette(p.active)}
  >
    {#if p.icon}
      <p.icon size={11} />
    {/if}
    <span>{p.name}</span>
    <span class="tabular-nums opacity-70">{p.count}</span>
  </button>
  <button
    type="button"
    aria-label={`${p.name} options`}
    aria-haspopup="menu"
    aria-expanded={isMenuOpen(p.kind, p.id)}
    onclick={(e) => openPillMenu(e, p.kind, p.id, p.name)}
    class={kbdTriggerClasses}
  >
    <Ellipsis size={12} />
  </button>
{/snippet}

<!-- Capped height: a long tag list scrolls here instead of pushing the
     table off screen. -->
<div
  id="filter-pills"
  role="group"
  aria-label={zone === 'projects' ? 'Project filters' : 'Star and tag filters'}
  class="flex max-h-28 flex-wrap items-center gap-1.5 overflow-y-auto py-0.5"
>
  {#if zone === 'projects'}
    {#each projects.items as p (p.id)}
      {@render pill({
        kind: 'project',
        id: p.id,
        name: p.name,
        count: p.paper_count,
        active: filters.project === p.id,
        palette: projectPillClasses,
        icon: Bookmark,
        onToggle: () => onProjectPill(p.id),
      })}
    {/each}
    <NewProjectInput
      onCreate={(name) => createNewProject(name)}
      inputClass="w-28 rounded-full border border-dashed border-indigo-600/40 bg-paper px-2 py-0.5 text-xs outline-none focus:border-indigo-600 dark:border-indigo-400/40 dark:bg-stone-800"
      buttonClass="inline-flex items-center rounded-full border border-dashed border-stone-300 px-2 py-0.5 text-xs text-stone-400 hover:border-stone-400 hover:text-stone-600 dark:border-stone-600 dark:text-stone-500 dark:hover:border-stone-500 dark:hover:text-stone-300"
    />
  {:else}
    <button
      type="button"
      aria-pressed={filters.starred === true}
      onclick={onStarPill}
      class={starPillClasses(filters.starred === true)}
    >
      <Star size={11} fill="currentColor" />
      <span>Starred</span>
    </button>
    {#each tags.items as t (t.id)}
      {@render pill({
        kind: 'tag',
        id: t.id,
        name: t.name,
        count: t.paper_count,
        active: filters.tag === t.name,
        palette: tagPillClasses,
        onToggle: () => onTagPill(t.name),
      })}
    {/each}
  {/if}
</div>

{#if openMenu}
  <!-- Keyed on the open record (a fresh object per open): reopening on
       another pill remounts the menu, resetting mode/busy/error to a fresh
       action list. -->
  {#key openMenu}
    <PillMenu
      kind={openMenu.kind}
      name={openMenu.name}
      x={menuX}
      y={menuY}
      onRename={renamePill}
      onDelete={deletePill}
      onClose={closeMenu}
    />
  {/key}
{/if}
