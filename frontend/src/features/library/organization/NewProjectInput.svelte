<script lang="ts">
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
  let input = $state<HTMLInputElement | null>(null);

  $effect(() => {
    if (adding) input?.focus();
  });

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
    } else if (e.key === 'Escape') {
      e.preventDefault();
      cancel();
    }
  }
</script>

{#if adding}
  <input
    bind:this={input}
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
