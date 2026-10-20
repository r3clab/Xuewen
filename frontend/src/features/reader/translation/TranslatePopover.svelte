<script lang="ts">
  import { closeTranslate, requestTranslate, translateBox } from './state.svelte';
  import { copyText } from '$shared/browser/clipboard';
  import { appSettings } from '$shared/api/settings.svelte';
  import { openDock } from '$features/reader/dock/state.svelte';
  import { chat } from '$features/reader/ask/chat.svelte';
  import { clickOutside } from '$shared/ui/clickOutside';
  import { anchoredPosition } from '$shared/ui/popoverPosition';
  import { toast } from '$shared/ui/toasts.svelte';
  import type { TranslateProvider } from '$shared/api/generated/TranslateProvider';

  const providers = $derived(appSettings.translate.providers ?? []);
  const showSwitch = $derived(providers.length > 1);
  const targetLabel = $derived(appSettings.translate.target_lang ?? 'zh');
  const sourceLabel = $derived(translateBox.sourceLang ? translateBox.sourceLang.toUpperCase() : null);
  const directionLabel = $derived(sourceLabel ? `${sourceLabel} → ${targetLabel}` : `→ ${targetLabel}`);

  // Clamp to the viewport (shared flip/clamp policy — see popoverPosition).
  const pos = $derived(anchoredPosition(translateBox.x, translateBox.y, { maxW: 320, belowOffset: 12 }));

  function pick(p: TranslateProvider) {
    if (p === translateBox.provider) return;
    void requestTranslate(translateBox.source, { x: translateBox.x, y: translateBox.y }, p);
  }
  async function doCopy() {
    try {
      await copyText(translateBox.translation);
      toast('success', 'Translation copied');
    } catch {
      toast('error', "Couldn't copy");
    }
  }
  function askAbout() {
    chat.draft = `Explain this passage:\n\n"${translateBox.source}"`;
    openDock('ask');
    closeTranslate();
  }
  function onWindowKeydown(e: KeyboardEvent) {
    if (translateBox.open && e.key === 'Escape') closeTranslate();
  }
</script>

<svelte:window onkeydown={onWindowKeydown} onscroll={closeTranslate} />

{#if translateBox.open}
  <div
    use:clickOutside={closeTranslate}
    role="dialog"
    aria-label="Translation"
    class="fixed z-50 w-80 max-w-[calc(100vw-1rem)] rounded-xl border border-stone-200 bg-paper shadow-2xl dark:border-stone-800 dark:bg-soot"
    style:left="{pos.left}px"
    style:top="{pos.top}px"
    style:transform={pos.below ? 'none' : 'translateY(-100%)'}
  >
    <div class="flex items-center gap-2 border-b border-stone-200 px-3 py-2 dark:border-stone-800">
      <span class="font-serif text-amber-700 dark:text-amber-500">譯</span>
      <span class="text-xs text-stone-500 dark:text-stone-400">{directionLabel}</span>
      <span class="flex-1"></span>
      {#if showSwitch}
        <span class="inline-flex overflow-hidden rounded-lg border border-stone-300 text-[11px] dark:border-stone-600">
          {#each providers as p (p)}
            <button
              type="button"
              aria-pressed={translateBox.provider === p}
              onclick={() => pick(p)}
              class={`px-2 py-0.5 focus-visible:outline focus-visible:outline-2 focus-visible:outline-amber-700 ${translateBox.provider === p ? 'bg-amber-700/10 text-amber-700 dark:text-amber-500' : 'text-stone-500'}`}
            >{p === 'llm' ? 'LLM' : 'DeepL'}</button>
          {/each}
        </span>
      {/if}
      <button
        type="button"
        aria-label="Close"
        onclick={closeTranslate}
        class="rounded p-0.5 text-stone-400 hover:text-stone-600 focus-visible:outline focus-visible:outline-2 focus-visible:outline-amber-700"
      >✕</button>
    </div>
    <div class="px-3 py-2.5">
      <p class="font-serif text-xs italic leading-snug text-stone-500 dark:text-stone-400">{translateBox.source}</p>
      <div class="my-2 h-px bg-stone-200 dark:bg-stone-800"></div>
      {#if translateBox.loading}
        <p class="text-sm text-stone-400">Translating…</p>
      {:else if translateBox.error}
        <p class="text-sm text-red-600 dark:text-red-400">Translation failed — check the translate provider config.</p>
      {:else}
        <p class="font-serif text-[15px] leading-relaxed text-ink dark:text-stone-100">{translateBox.translation}</p>
        <div class="mt-2.5 flex items-center gap-2">
          <button
            type="button"
            onclick={() => void doCopy()}
            class="inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1 text-xs font-medium text-amber-700 hover:bg-amber-700/10 focus-visible:outline focus-visible:outline-2 focus-visible:outline-amber-700 dark:border-stone-700 dark:text-amber-500"
          >⧉ Copy</button>
          {#if chat.available}
            <button
              type="button"
              onclick={askAbout}
              class="inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1 text-xs font-medium text-amber-700 hover:bg-amber-700/10 focus-visible:outline focus-visible:outline-2 focus-visible:outline-amber-700 dark:border-stone-700 dark:text-amber-500"
            >問 Ask about this</button>
          {/if}
        </div>
      {/if}
    </div>
  </div>
{/if}
