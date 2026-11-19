import { localEngine } from '@embedpdf/engine';
import { annotationPlugin } from '@embedpdf/svelte/annotation';
import { historyPlugin } from '@embedpdf/svelte/history';
import { interactionPlugin } from '@embedpdf/svelte/interaction';
import { linkPlugin } from '@embedpdf/svelte/link';
import { outlinePlugin } from '@embedpdf/svelte/outline';
import { renderPlugin } from '@embedpdf/svelte/render';
import { createCapabilityToken, type AnyPlugin, type Permission } from '@embedpdf/svelte/runtime';
import { searchPlugin } from '@embedpdf/svelte/search';
import { selectionPlugin } from '@embedpdf/svelte/selection';
import { stagePlugin, type StageCapability } from '@embedpdf/svelte/stage';
import { pageTextPlugin } from './pageText';

/// PDFium runs in the package's own worker, and both the worker script and
/// `embedpdf.wasm` are referenced through `new URL(…, import.meta.url)`, so
/// Vite emits them under the fingerprinted `assets/` of this build: nothing is
/// fetched from a CDN, the reader works offline, and the backend serves the
/// 8 MB wasm `immutable` (cache_control_for in src/web/assets.rs).
///
/// The default Hebrew/Arabic fallback fonts are left out, as 2.x's
/// `fontFallback: null` left out every font fetch: a glyph a paper doesn't
/// embed renders as the engine's own substitute.
export const ENGINE_OPTIONS = { defaultFallbackFonts: false } as const;

export function createEngine() {
  return localEngine(ENGINE_OPTIONS);
}

/// What every document opens with. Without `pdf.permissions` the engine
/// ignores a file's own restriction bits, so this list is exactly what the
/// reader may do, except that `pdf.permissions` is in it: a file that forbids
/// commenting or printing keeps that.
///
/// `doc.text.copy` is the one bit Xuewen overrides on purpose. A PDF can carry
/// a "no copying" flag, but the same file's full text is already extracted by
/// `pdftotext` at ingest, stored in SQLite, indexed into both search indexes,
/// summarised by an LLM, and written out as `<library_root>/agent/<paper_id>/
/// paper.txt` for the Ask agent to read; refusing to let the reader copy one
/// sentence out of a document the app has read cover to cover would be
/// theatre. It is what ⌘C (pdfCopy.ts), selection-translate (PdfPages.svelte),
/// the citation text reads and a mark's quoted text (pdf/pageText.ts) need.
///
/// `doc.annotate.import` lets the sidecar's marks come back under their own
/// names and dates (annotations/sync.ts); `doc.download` is the annotated
/// export (annotations/export.ts). No PDF flag governs either.
export const DOCUMENT_SCOPE = ['pdf.permissions', 'doc.text.copy', 'doc.annotate.import', 'doc.download'] as const satisfies readonly (Permission | 'pdf.permissions')[];

/// Shown as the author of every mark, and so in an exported copy. Deliberately
/// not the OS user: nothing here should leak a real name into a file the user
/// shares.
export const IDENTITY = { displayName: 'Xuewen' } as const;

/// The side panel's page strip: a second view of each document, with its own
/// camera and zoom.
export const ThumbsToken = createCapabilityToken<StageCapability>('stage-thumbs');

export const THUMB_WIDTH = 150;
export const THUMB_LABEL_HEIGHT = 16;

/// The plugins for the single, app-level viewer. Documents are opened per tab
/// at runtime (PdfDeck), never as `initialDocuments`.
export function viewerPlugins(): AnyPlugin[] {
  return [
    // The viewer fills the window: there is no page around it for a scroll
    // past the last page to chain into, only the browser's back gesture.
    stagePlugin({ zoom: { mode: 'fit-page' }, scrollChaining: false }),
    stagePlugin({
      id: 'stage-thumbs',
      token: ThumbsToken,
      zoom: { pageWidth: THUMB_WIDTH },
      padding: 8,
      gap: { px: 10 },
      pageFrame: { bottom: THUMB_LABEL_HEIGHT },
      // A drag on the strip scrolls it; it never selects text or draws.
      interaction: false,
      zoomGestures: false,
      scrollChaining: false,
      responsive: [],
    }),
    renderPlugin(),
    interactionPlugin(),
    selectionPlugin(),
    searchPlugin({ highlight: { color: 'rgba(180, 83, 9, 0.28)', activeColor: 'rgba(180, 83, 9, 0.55)' } }),
    outlinePlugin(),
    // Registered for what it does to the annotation layer, never mounted as a
    // <LinkLayer>: while the pointer tool is active, links belong to the link
    // plugin, so the annotation plane neither selects nor moves them. The
    // reader's own CitationLayer is the only thing that answers a click on
    // one — following a link is never the plugin's to do here.
    linkPlugin(),
    // Undo and redo for marks: every annotation change this session makes is
    // one step, per document.
    historyPlugin(),
    // Marks are written into the in-memory document only; the library file is
    // never written, because nothing here ever downloads the open document.
    // The sidecar (annotations/sync.ts) is where they are kept. A new mark is
    // selected and the tool stays armed, for marking passages in a row.
    annotationPlugin({ afterCreate: { select: true, tool: 'stay' } }),
    pageTextPlugin,
  ];
}
