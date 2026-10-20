<script lang="ts">
  // The one loading indicator: icon + label, announced as a status region.
  // Callers own the surrounding layout (padding, centering).
  import { Loader } from 'lucide-svelte';

  let { label = 'Loading…', class: cls = '' }: { label?: string; class?: string } = $props();
</script>

<div role="status" class={`flex items-center gap-2 text-sm text-stone-500 dark:text-stone-400 ${cls}`}>
  <Loader size={14} class="shrink-0 animate-spin" aria-hidden="true" />
  <span>{label}</span>
</div>
