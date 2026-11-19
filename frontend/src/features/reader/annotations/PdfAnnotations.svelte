<script lang="ts">
  import { Download, Trash } from '@lucide/svelte';
  import { useDocuments, useKernel } from '@embedpdf/svelte/runtime';
  import { AnnotationToken, useAnnotation } from '@embedpdf/svelte/annotation';
  import { useStage } from '@embedpdf/svelte/stage';
  import { annotationList, annotations, removeAnnotation } from './store.svelte';
  import { KIND_LABELS } from './adapter';
  import { buildAnnotatedPdf, exportErrorMessage } from './export';
  import { colorHex, colorLabel } from './palette';
  import { annotatedFilename, downloadBlob } from './download';
  import { pdfUrl } from '$features/reader/pdf/api';
  import { viewer } from '$features/reader/tabs/state.svelte';
  import { toast } from '$shared/ui/toasts.svelte';
  import type { Annotation } from '$shared/api/generated/Annotation';

  let { documentId }: { documentId: string } = $props();

  const stage = useStage();
  const annotation = useAnnotation();
  const documents = useDocuments();
  const kernel = useKernel();

  // The tab id is the paper id (see PdfDeck).
  const items = $derived(annotationList(documentId));
  const failed = $derived(annotations.error[documentId] ?? null);
  const title = $derived(viewer.tabs.find((t) => t.id === documentId)?.title ?? 'paper');

  /// A row's id is its mark's name in the document (see annotations/sync.ts).
  function markOf(a: Annotation) {
    return annotation.list().find((m) => m.nm === a.id) ?? null;
  }

  /// Jump to the mark's page and select it, so the reader can see which one
  /// the panel row refers to. Selecting is best-effort: a row whose payload
  /// could not be rebuilt was never restored into the document, so there is
  /// nothing on the page to select — the page jump still works.
  function jump(a: Annotation): void {
    stage.goToPage(a.page_index);
    const mark = markOf(a);
    if (mark) annotation.selection.select([mark.ref]);
  }

  /// Deleting through the plugin (rather than the store) is what keeps the
  /// drawn mark and the row in step: the plugin emits a delete event, and the
  /// sync loop removes the row. A mark that never made it into the document
  /// has no plugin copy, so that one is removed directly.
  async function remove(a: Annotation): Promise<void> {
    const mark = markOf(a);
    try {
      if (mark) await annotation.delete(mark.ref);
      else await removeAnnotation(documentId, a.id);
    } catch (e) {
      toast('error', e instanceof Error ? e.message : String(e));
    }
  }

  let exporting = $state(false);

  /// Save a copy of the PDF with the marks burned in — for sending to someone
  /// who does not have this library. The library's own file is untouched; see
  /// annotations/export.ts for how.
  async function exportAnnotated(): Promise<void> {
    if (exporting) return;
    exporting = true;
    try {
      const blob = await buildAnnotatedPdf(documentId, items, {
        open: (id) => documents.open({ kind: 'url', url: pdfUrl(documentId), id }, { activate: false }),
        close: (id) => documents.close(id),
        pages: (id) => documents.listPages(id),
        scope: (id) => kernel.tryCapability(AnnotationToken, id),
        download: async (id) => new Uint8Array(await documents.download(id)),
      });
      downloadBlob(blob, annotatedFilename(title));
    } catch (e) {
      toast('error', exportErrorMessage(e));
    } finally {
      exporting = false;
    }
  }
</script>

<div class="flex min-h-0 flex-1 flex-col">
  <div class="min-h-0 flex-1 overflow-y-auto p-1.5">
    {#if failed}
      <p class="px-1 py-3 text-xs text-stone-500 dark:text-stone-400">
        Annotations could not be loaded — {failed}
      </p>
    {:else if items.length === 0}
      <p class="px-1 py-3 text-xs text-stone-500 dark:text-stone-400">
        No annotations yet. Select text, then pick a tool from the toolbar.
      </p>
    {:else}
      <ul class="space-y-1">
        {#each items as a (a.id)}
          <li class="group relative">
            <button
              type="button"
              class="w-full rounded-lg border border-transparent px-2 py-1.5 text-left hover:border-stone-200 hover:bg-parchment dark:hover:border-stone-700 dark:hover:bg-stone-800"
              onclick={() => jump(a)}
            >
              <span class="flex items-center gap-1.5 text-[10px] text-stone-500 dark:text-stone-400">
                <span
                  class="h-2.5 w-2.5 shrink-0 rounded-full"
                  style:background-color={colorHex(a.color)}
                  aria-hidden="true"
                ></span>
                <span>{KIND_LABELS[a.kind]}</span>
                <span aria-hidden="true">·</span>
                <span>p.{a.page_index + 1}</span>
                <span class="sr-only">{colorLabel(a.color)}</span>
              </span>
              {#if a.quoted_text}
                <span
                  class="mt-0.5 block border-l-2 pl-1.5 font-serif text-xs leading-snug text-ink dark:text-stone-200"
                  style:border-color={colorHex(a.color)}
                >
                  {a.quoted_text}
                </span>
              {/if}
              {#if a.note}
                <span class="mt-1 block text-xs italic leading-snug text-stone-600 dark:text-stone-300">
                  {a.note}
                </span>
              {/if}
              {#if !a.quoted_text && !a.note}
                <span class="mt-0.5 block text-xs text-stone-400 dark:text-stone-500">
                  (no text)
                </span>
              {/if}
            </button>
            <!-- Sits over the row rather than inside it: a button inside a
                 button is invalid HTML and the inner one never gets its click. -->
            <button
              type="button"
              class="absolute right-1 top-1 rounded p-1 text-stone-400 opacity-0 hover:bg-stone-200 hover:text-ink focus:opacity-100 group-hover:opacity-100 dark:hover:bg-stone-700 dark:hover:text-stone-100"
              aria-label={`Delete ${KIND_LABELS[a.kind]} on page ${a.page_index + 1}`}
              title="Delete"
              onclick={() => void remove(a)}
            >
              <Trash size={12} />
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
  {#if items.length > 0}
    <div class="border-t border-stone-200 p-1.5 dark:border-stone-800">
      <button
        type="button"
        class="flex w-full items-center justify-center gap-1.5 rounded-lg px-2 py-1.5 text-xs text-stone-600 hover:bg-parchment hover:text-ink disabled:opacity-50 disabled:hover:bg-transparent dark:text-stone-300 dark:hover:bg-stone-800"
        title="Save a copy of the PDF with these marks in it — the library's own file is left as it is"
        disabled={exporting}
        onclick={() => void exportAnnotated()}
      >
        <Download size={13} />
        {exporting ? 'Exporting…' : 'Export annotated PDF'}
      </button>
    </div>
  {/if}
</div>
