<!--
  Dropdown scaffold for the reader toolbar's menus (zoom presets, annotation
  tools): click-outside and Escape dismissal, focus moving into the menu and
  back to its trigger, and arrow-key navigation between items live here once.
  Each menu supplies its own trigger button — which keeps its per-menu
  classes, aria-expanded, and toggle — and panel contents, and owns the bound
  `open` state (the toolbar feeds it into the pill's auto-hide hold).
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { Attachment } from 'svelte/attachments';
  import { clickOutside } from '$shared/ui/clickOutside';
  import { escapeLayer } from '$shared/ui/escape';
  import { returnFocus } from '$shared/ui/focus';
  import { menuItems, menuNavKeydown } from '$shared/ui/menuNav';

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

  const focusFirstItem: Attachment<HTMLElement> = (menu) => {
    menuItems(menu)[0]?.focus();
  };
</script>

<div
  class="relative"
  use:clickOutside={() => {
    if (open) open = false;
  }}
>
  {@render trigger()}
  {#if open}
    <div
      {@attach returnFocus}
      {@attach focusFirstItem}
      {@attach escapeLayer(() => (open = false))}
      role="menu"
      aria-label={label}
      tabindex="-1"
      onkeydown={(e) => menuNavKeydown(e.currentTarget, e)}
      class={`absolute left-1/2 top-full z-30 mt-1.5 -translate-x-1/2 rounded-xl border border-stone-200 bg-paper/95 shadow-lg backdrop-blur dark:border-stone-800 dark:bg-soot/95 ${panelClass}`}
    >
      {@render children()}
    </div>
  {/if}
</div>
