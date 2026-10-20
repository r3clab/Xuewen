<script lang="ts">
  import { chat } from '$features/reader/ask/chat.svelte';
  import { pillMotionStyle } from './pillStyles';
  import { dock, toggleDock } from '$features/reader/dock/state.svelte';
  import type { PillHide } from './pillHide.svelte';

  let { pill }: { pill: PillHide } = $props();

  // One seal for the one panel — the reader's trigger speaks the app's own
  // language (學問, 譯). 問 while Ask is configured, 詳 when it isn't, since
  // the panel is then the record alone. Zen keeps `z` and the panel header;
  // it was never worth a permanent mark over the page.
  const btn =
    'rounded-lg px-1.5 py-1 font-serif text-base leading-none hover:bg-parchment dark:hover:bg-stone-800';

  const hidden = $derived(dock.open || !pill.visible);
</script>

<!-- svelte-ignore a11y_interactive_supports_focus -- every control inside
     the pill is individually tabbable via normal document tab order; the
     toolbar container itself is not a tab stop (same rationale as
     PdfToolbar's pill). -->
<div
  role="toolbar"
  aria-label="Reader quick actions"
  onpointerenter={() => pill.pillEnter()}
  onpointerleave={() => pill.pillLeave()}
  onfocusin={() => pill.focusIn()}
  onfocusout={() => pill.focusOut()}
  style={pillMotionStyle(!hidden)}
  class={`absolute right-3 top-3 z-20 flex items-center gap-1 rounded-xl border border-stone-200 bg-paper/90 px-1.5 py-1 shadow backdrop-blur dark:border-stone-800 dark:bg-soot/90 ${
    hidden ? 'pointer-events-none opacity-0' : 'opacity-100'
  }`}
>
  <button
    type="button"
    class={`${btn} ${
      chat.available
        ? 'text-amber-700 dark:text-amber-500'
        : 'text-stone-600 hover:text-ink dark:text-stone-300'
    }`}
    aria-label="Paper panel"
    title={chat.available ? 'Paper panel — i · ask c' : 'Paper panel — i'}
    onclick={() => toggleDock(chat.available ? 'ask' : 'record')}
  >{chat.available ? '問' : '詳'}</button>
</div>
