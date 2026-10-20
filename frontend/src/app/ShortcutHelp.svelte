<script lang="ts">
  import { SHORTCUT_GROUPS } from '$app/keys/keymap';
  import { shell } from './shell.svelte';
  import Modal from '$shared/ui/Modal.svelte';
</script>

<Modal title="Keyboard shortcuts" onclose={() => (shell.helpOpen = false)}>
  <div class="grid gap-5 sm:grid-cols-2">
    {#each SHORTCUT_GROUPS as g (g.title)}
      <section>
        <h3 class="text-caption font-semibold uppercase tracking-[.08em] text-stone-500 dark:text-stone-400">
          {g.title}
        </h3>
        <dl class="mt-2 space-y-1.5">
          {#each g.items as s (s.label)}
            <div class="flex items-center justify-between gap-4 text-sm">
              <dt class="text-stone-600 dark:text-stone-300">{s.label}</dt>
              <dd>
                <kbd class="rounded border border-stone-300 px-1.5 py-0.5 font-mono text-caption text-stone-500 dark:border-stone-700 dark:text-stone-400">
                  {s.keys}
                </kbd>
              </dd>
            </div>
          {/each}
        </dl>
      </section>
    {/each}
  </div>
</Modal>
