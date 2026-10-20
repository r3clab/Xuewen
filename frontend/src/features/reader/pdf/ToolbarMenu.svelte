<!--
  Dropdown scaffold for the reader toolbar's menus (zoom presets, annotation
  tools): click-outside and Escape dismissal plus the positioned role=menu
  panel live here once. Each menu supplies its own trigger button — which
  keeps its per-menu classes, aria-expanded, and toggle — and panel contents,
  and owns the bound `open` state (the toolbar feeds it into the pill's
  auto-hide hold).
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import { clickOutside } from '$shared/ui/clickOutside';

  let {
    open = $bindable(false),
    label,
    panelClass,
    trigger,
    children,
  }: {
    open?: boolean;
    label: string;
    /// Per-menu width/padding, appended to the shared panel classes.
    panelClass: string;
    trigger: Snippet;
    children: Snippet;
  } = $props();
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -- the keydown only
     carries Escape while the menu is open, and lives on the wrapper — not
     the panel — because the trigger button keeps focus after opening; a
     handler on the sibling panel would never see that Escape. Every
     interactive child is a real button. -->
<div
  class="relative"
  use:clickOutside={() => {
    if (open) open = false;
  }}
  onkeydown={(e) => {
    if (open && e.key === 'Escape') {
      e.stopPropagation(); // the global cascade must not see this
      open = false;
    }
  }}
>
  {@render trigger()}
  {#if open}
    <div
      role="menu"
      aria-label={label}
      class={`absolute left-1/2 top-full z-30 mt-1.5 -translate-x-1/2 rounded-xl border border-stone-200 bg-paper/95 shadow-lg backdrop-blur dark:border-stone-800 dark:bg-soot/95 ${panelClass}`}
    >
      {@render children()}
    </div>
  {/if}
</div>
