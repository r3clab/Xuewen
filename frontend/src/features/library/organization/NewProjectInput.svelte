<script lang="ts">
  import { escapeLayer } from '$shared/ui/escape';
  import { focusOnMount } from '$shared/ui/focus';

  /// The dashed "+ New project" pill that flips into an inline name input
  /// (Enter/blur submits, Escape cancels) — shared by the filter strip's
  /// project pills and the paper detail's project editor, which differ only in pill
  /// shape (hence the class props).
  let {
    onCreate,
    inputClass,
    buttonClass,
  }: {
    onCreate: (name: string) => void | Promise<unknown>;
    inputClass: string;
    buttonClass: string;
  } = $props();

  let adding = $state(false);
  let name = $state('');

  function start() {
    name = '';
    adding = true;
  }
  function cancel() {
    adding = false;
    name = '';
  }
  function submit() {
    const trimmed = name.trim();
    cancel();
    if (trimmed) void onCreate(trimmed);
  }
  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      submit();
    }
  }
</script>

{#if adding}
  <input
    {@attach focusOnMount}
    {@attach escapeLayer(cancel)}
    bind:value={name}
    type="text"
    aria-label="New project name"
    placeholder="Project name"
    onkeydown={onKeydown}
    onblur={() => (name.trim() ? submit() : cancel())}
    class={inputClass}
  />
{:else}
  <button type="button" onclick={start} class={buttonClass}>+ New project</button>
{/if}
