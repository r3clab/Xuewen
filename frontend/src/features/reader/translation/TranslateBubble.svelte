<script lang="ts">
  // Manual-mode affordance: shown at the last pointer-up position after a
  // finished text selection, when [translate].trigger is 'manual'. Clicking it
  // (rather than auto-translating every selection) is the whole point of
  // manual mode — see translation/state.svelte.ts's translateTrigger().
  let { x, y, onTranslate }: { x: number; y: number; onTranslate: () => void } = $props();
</script>

<button
  type="button"
  onclick={onTranslate}
  style:left="{x}px"
  style:top="{y}px"
  class="fixed z-50 inline-flex -translate-x-1/2 -translate-y-full items-center gap-1.5 rounded-full border border-amber-700/40 bg-paper px-2.5 py-1 text-xs font-semibold text-amber-700 shadow-lg focus-visible:outline focus-visible:outline-2 focus-visible:outline-amber-700 dark:border-amber-500/40 dark:bg-soot dark:text-amber-500"
>譯 Translate</button>
