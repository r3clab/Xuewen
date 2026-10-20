/// Renderer overrides handed to `<AnnotationLayer>`.
///
/// The layer merges these with its built-ins by id: an external entry replaces
/// the built-in that claims the same id, and the rest are appended. So the way
/// to *remove* a built-in renderer is to claim its id and match nothing.

import type { PdfAnnotationObject } from '@embedpdf/models';
import { createRenderer } from '@embedpdf/plugin-annotation/svelte';

/// This reader already owns link annotations: `CitationLayer` turns them into
/// reference popovers, positioned from the same rects. The plugin's built-in
/// `link` renderer would draw a second overlay on top of those, and whichever
/// won the hit test would swallow the other's clicks. Claiming the id with a
/// matcher that never fires leaves link annotations with no renderer at all,
/// which is what we want — the citation layer is the only thing drawing there.
///
/// Version-pinned to the merge rule in @embedpdf/plugin-annotation ~2.14.4. If
/// the merge ever became additive instead of id-replacing, links would get
/// their built-in overlay back and citation popovers would start missing
/// clicks — visible immediately, and no worse than before this feature.
export const SUPPRESS_LINK_RENDERER = createRenderer({
  id: 'link',
  // Spelled out rather than left to `createRenderer`'s default, so the intent
  // survives a reader who doesn't know what the default is.
  matches: (_a: PdfAnnotationObject): _a is PdfAnnotationObject => false,
});

/// Everything `<AnnotationLayer>` should be given, in one place.
export const ANNOTATION_RENDERERS = [SUPPRESS_LINK_RENDERER];
