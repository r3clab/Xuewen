import { createPluginRegistration } from '@embedpdf/core';
import type { PluginBatchRegistrations, PluginRegistryConfig } from '@embedpdf/core';
import { ViewportPluginPackage } from '@embedpdf/plugin-viewport';
import { ScrollPluginPackage } from '@embedpdf/plugin-scroll';
import { RenderPluginPackage } from '@embedpdf/plugin-render';
import { SelectionPluginPackage } from '@embedpdf/plugin-selection';
import { InteractionManagerPluginPackage } from '@embedpdf/plugin-interaction-manager';
import { DocumentManagerPluginPackage } from '@embedpdf/plugin-document-manager';
import { ZoomPluginPackage, ZoomMode } from '@embedpdf/plugin-zoom';
import { TilingPluginPackage } from '@embedpdf/plugin-tiling';
import { SearchPluginPackage } from '@embedpdf/plugin-search';
import { ThumbnailPluginPackage } from '@embedpdf/plugin-thumbnail';
import { BookmarkPluginPackage } from '@embedpdf/plugin-bookmark';
import { AnnotationPluginPackage } from '@embedpdf/plugin-annotation/svelte';
import { HistoryPluginPackage } from '@embedpdf/plugin-history';
import { colorPatch, TOOL_BY_KIND } from '$features/reader/annotations/adapter';
import { DEFAULT_ANNOTATION_COLOR, colorHex } from '$features/reader/annotations/palette';
import type { AnnotationKind } from '$shared/api/generated/AnnotationKind';
// `?url` so Vite emits the wasm as a content-hashed asset and hands back its
// path — see ENGINE_OPTIONS below for why the fingerprint matters. The package
// exports this subpath explicitly ("./pdfium.wasm" in its package.json), so
// this is a supported entry point, not a reach into its internals.
import pdfiumWasmUrl from '@embedpdf/pdfium/pdfium.wasm?url';

// Load-bearing offline config:
//  - worker:true   -> PDFium runs in EmbedPDF's stock blob module worker. The
//    worker's self.location is a blob: URL, which cannot resolve a
//    path-absolute fetch like '/pdfium.wasm' (Chromium throws "Failed to
//    parse URL from /pdfium.wasm" — there's no hierarchical path on a blob:
//    base to graft it onto). Passing a fully-qualified URL sidesteps that
//    entirely, since it needs no base-relative resolution. That is why the
//    imported path still goes through `new URL(..., location.origin)`.
//  - wasmUrl       -> self-hosted (the default is a jsDelivr CDN, which breaks
//    offline), and emitted under Vite's fingerprinted `assets/` rather than
//    copied verbatim into `public/`. The content hash in the name is what lets
//    the backend serve it `immutable` (see cache_control_for in
//    src/web/assets.rs): 4.6 MB fetched once per build instead of revalidated
//    on every page load, which is the single largest asset the reader needs.
//  - fontFallback:null -> no external font fetches
export const ENGINE_OPTIONS = {
  wasmUrl: new URL(pdfiumWasmUrl, location.origin).href,
  worker: true,
  fontFallback: null,
} as const;

/// Registry-wide config for <EmbedPDF>. The only thing in it is one permission
/// override. A PDF can carry a "no copying" bit, and the selection plugin
/// honours it at a single choke point: `getSelectedText` rejects with
/// PdfErrorCode.Security before doing anything else (plugin-selection 2.14.4
/// index.js:1281). Xuewen does not honour it, on purpose. The same file's full
/// text is already extracted by `pdftotext` at ingest, stored in SQLite,
/// indexed into Tantivy and Qdrant, summarised by an LLM, and written out as
/// `<library_root>/agent/<paper_id>/paper.txt` for the Ask agent to read;
/// refusing to let the reader copy one sentence out of a document the app has
/// read cover to cover would be theatre. The bit is advisory metadata, not
/// encryption, and PDFium reports AllowAll for unencrypted files, so this only
/// ever changes behaviour for the rare restricted paper — which is the user's
/// own library copy.
///
/// Blast radius, since that one choke point has three callers here: ⌘C
/// (pdfCopy.ts), selection-translate (PdfPages.svelte), and the annotation
/// plugin's quoted-text capture, which fills a highlight's `quoted_text` in the
/// sidecar. On a restricted PDF all three go from silently inert to working.
/// Only copyContents is overridden — print/modifyAnnotations/assemble keep
/// whatever the document says.
export const REGISTRY_CONFIG: PluginRegistryConfig = {
  permissions: { overrides: { copyContents: true } },
};

// One shared registry hosts every open paper as a document (EmbedPDF's Svelte
// bindings use a module-level singleton context, so there can only be ONE
// <EmbedPDF> per page). `maxDocuments` caps how many tabs can be open at once.
const MAX_OPEN_DOCUMENTS = 32;

/**
 * Plugin registrations for the single, app-level viewer. Documents are opened
 * per tab at runtime via the document-manager capability (no `initialDocuments`).
 */
export function viewerPlugins(): PluginBatchRegistrations {
  return [
    createPluginRegistration(DocumentManagerPluginPackage, {
      maxDocuments: MAX_OPEN_DOCUMENTS,
    }),
    createPluginRegistration(ViewportPluginPackage),
    createPluginRegistration(ScrollPluginPackage),
    createPluginRegistration(RenderPluginPackage),
    createPluginRegistration(InteractionManagerPluginPackage),
    // Deliberately the BASE package, not '@embedpdf/plugin-selection/svelte' —
    // do not "fix" the asymmetry with AnnotationPluginPackage below. The
    // /svelte build wraps this one with an auto-mounted CopyToClipboard
    // utility, the only clipboard write in the whole EmbedPDF tree, which calls
    // navigator.clipboard.writeText bare and merely console.errors a rejection:
    // no execCommand fallback, so it is dead over plain HTTP to a LAN host
    // (`--allow-remote`), which is exactly why copyText exists. ⌘C goes through
    // pdfCopy.ts + copyText instead, and nothing calls the plugin's own
    // copyToClipboard(), so mounting that utility would only add a second,
    // worse clipboard writer.
    createPluginRegistration(SelectionPluginPackage),
    createPluginRegistration(ZoomPluginPackage, { defaultZoomLevel: ZoomMode.FitPage }),
    // Visible-area high-res tiles; the full-page RenderLayer base stays at
    // scale 1 so zooming never re-renders whole pages (see PdfPages.svelte).
    // Defaults (tileSize 768) match the ready-made viewer; only pass config
    // here if a verified option needs changing.
    createPluginRegistration(TilingPluginPackage),
    // Toolbar features: find-in-document, page thumbnails, document outline.
    createPluginRegistration(SearchPluginPackage),
    // autoScroll would snap the pane to the current page on EVERY page
    // change, fighting manual thumbnail browsing (trackpad momentum keeps
    // firing page changes for a second after a flick). The side panel
    // positions the pane once on open instead, via a direct scrollTop write
    // (see PdfSidePanel.svelte) — scrollToThumb/scrollTo$ is never called,
    // so scrollBehavior never gets consulted. `scrollBehavior: 'auto'` is
    // kept anyway as spec-mandated defense-in-depth: nothing in this app
    // emits through that scroll channel anymore.
    createPluginRegistration(ThumbnailPluginPackage, { autoScroll: false, scrollBehavior: 'auto' }),
    createPluginRegistration(BookmarkPluginPackage),
    // Annotations are a SQLite sidecar, never a write to the PDF — see
    // ANNOTATION_OPTIONS. History is the annotation plugin's optional
    // dependency: registering it is all that's needed for undo/redo, which the
    // plugin files under the 'annotations' topic (ANNOTATION_HISTORY_TOPIC,
    // which lives in annotations/commands.ts — see the note there on why that
    // module must not import this one).
    createPluginRegistration(HistoryPluginPackage),
    createPluginRegistration(AnnotationPluginPackage, ANNOTATION_OPTIONS),
  ];
}

/// Every tool we surface, seeded with the default palette color. The color is a
/// live preference, so the toolbar pushes changes through `setToolDefaults`
/// rather than this being the last word.
///
/// The plugin's own defaults are otherwise left alone. In particular a
/// highlight keeps `opacity: 1` with `blendMode: Multiply` — that multiply is
/// what lets the text show through, and swapping it for a translucent fill
/// washes the glyphs out instead.
function paletteTools(): { id: string; defaults: Record<string, string> }[] {
  const hex = colorHex(DEFAULT_ANNOTATION_COLOR);
  return Object.entries(TOOL_BY_KIND).map(([kind, id]) => ({
    id,
    defaults: colorPatch(kind as AnnotationKind, hex),
  }));
}

export const ANNOTATION_OPTIONS = {
  // The one setting this whole feature rests on. With autoCommit the plugin
  // would write marks back into the PDF through PDFium, changing the bytes
  // under `papers.content_hash` — the hash that drives ingest dedupe and the
  // `_unsorted/<hash>.pdf` name. Marks live in SQLite instead (see
  // src/annotations/), so the library file stays byte-identical forever.
  autoCommit: false,
  // The reader has its own citation popovers over link annotations
  // (CitationLayer). Letting the plugin open URIs on click would fire a
  // window.open behind that popover.
  autoOpenLinks: false,
  // Shown as the annotation's author in an exported copy. Deliberately not the
  // OS user: nothing here should leak a real name into a file the user shares.
  annotationAuthor: 'Xuewen',
  // A just-drawn mark is selected so its note editor and color swatches are one
  // click away, but the tool stays armed for marking several passages in a row.
  selectAfterCreate: true,
  deactivateToolAfterCreate: false,
  tools: paletteTools(),
} as const;
