<script lang="ts">
  import { onMount, type Component } from 'svelte';
  import { slide } from 'svelte/transition';
  import FilePicker from '$features/library/search/FilePicker.svelte';
  import IdentifyModal from '$features/identify/IdentifyModal.svelte';
  import ImportModal from '$features/importing/ImportModal.svelte';
  import LibraryView from '$features/library/LibraryView.svelte';
  import LeaderHint from './LeaderHint.svelte';
  import PaperContextMenu from '$features/library/table/PaperContextMenu.svelte';
  import ReaderDock from '$features/reader/dock/ReaderDock.svelte';
  import ShortcutHelp from './ShortcutHelp.svelte';
  import Spinner from '$shared/ui/Spinner.svelte';
  import TabBar from '$features/reader/tabs/TabBar.svelte';
  import Toaster from '$shared/ui/Toaster.svelte';
  import TopBar from './TopBar.svelte';
  import TranslatePopover from '$features/reader/translation/TranslatePopover.svelte';
  import { initApp } from './init';
  import { chat, loadThread } from '$features/reader/ask/chat.svelte';
  import { DUR, dur } from '$shared/ui/motion';
  import { handleKeydown } from '$app/keys/shortcuts';
  import { identifyState } from '$features/identify/state.svelte';
  import { importState } from '$features/importing/queue.svelte';
  import { finder } from '$features/library/search/state.svelte';
  import { viewer } from '$features/reader/tabs/state.svelte';
  import { dock } from '$features/reader/dock/state.svelte';
  import { shell } from './shell.svelte';

  onMount(initApp);

  // The chat thread follows the active paper while the dock is open — the
  // record and the thread share one panel, so there is no tab to gate on.
  $effect(() => {
    if (dock.open && chat.available && viewer.activeId) void loadThread(viewer.activeId);
  });

  // The PDF reader pulls in the entire @embedpdf/PDFium subtree — by far the
  // heaviest part of the bundle. Load it lazily (its own chunk) the first time
  // a paper is opened, so the library/search view no longer pays for it up
  // front. Once loaded it stays resolved for the session.
  let PdfViewer = $state<Component | null>(null);
  $effect(() => {
    if (viewer.activeId !== null && !PdfViewer) {
      void import('$features/reader/pdf/PdfViewer.svelte').then((m) => {
        PdfViewer = m.default;
      });
    }
  });
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="flex h-full flex-col bg-paper text-ink dark:bg-night dark:text-stone-100">
  {#if !viewer.zen}
    <div transition:slide={{ duration: dur(DUR.base) }}>
      <TopBar />
    </div>
  {/if}
  <main class="flex min-h-0 min-w-0 flex-1 flex-col">
    {#if !viewer.zen}
      <div transition:slide={{ duration: dur(DUR.base) }}>
        <TabBar />
      </div>
    {/if}
    <div class="flex min-h-0 flex-1">
      <!-- Reader column is hidden (not unmounted) while the Library view is
           active, so returning to an open paper doesn't rebuild the subtree.
           PdfDeck likewise keeps one PdfTab per open paper alive behind
           visibility:hidden, so scroll and zoom survive both a tab switch
           and a trip through the Library. -->
      <div class={`relative min-h-0 min-w-0 flex-1 ${viewer.activeId === null ? 'hidden' : 'flex'}`}>
        {#if PdfViewer}
          <PdfViewer />
        {:else if viewer.activeId !== null}
          <div class="flex flex-1 items-center justify-center">
            <Spinner label="Loading reader…" />
          </div>
        {/if}
        {#if dock.open && viewer.activeId}
          <!-- Non-interactive recede veil: the PDF stays scrollable underneath. -->
          <div class="pointer-events-none absolute inset-0 z-10 bg-ink/5 dark:bg-black/25" aria-hidden="true"></div>
          <ReaderDock id={viewer.activeId} />
        {/if}
      </div>
      {#if viewer.activeId === null}<LibraryView />{/if}
    </div>
  </main>
</div>
{#if importState.open}<ImportModal />{/if}
{#if identifyState.open}<IdentifyModal />{/if}
{#if finder.open}<FilePicker />{/if}
{#if shell.helpOpen}<ShortcutHelp />{/if}
<LeaderHint />

<PaperContextMenu />
<TranslatePopover />
<Toaster />
