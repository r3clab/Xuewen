/// Annotation tool preferences: which mark the next drag or click makes, and
/// in which color. Global rather than per-tab, matching how the side panel and
/// the PDF appearance mode already behave — picking a highlighter in one paper
/// leaves it picked when you switch to the next.

import {
  DEFAULT_ANNOTATION_COLOR,
  isAnnotationColor,
} from './palette';
import type { AnnotationColor } from '$shared/api/generated/AnnotationColor';
import type { AnnotationKind } from '$shared/api/generated/AnnotationKind';

export const annotationTools = $state<{
  /// The armed tool, or null for plain reading. Deliberately NOT persisted: a
  /// reload should never leave a highlighter armed with no visible cause.
  active: AnnotationKind | null;
  color: AnnotationColor;
}>({ active: null, color: DEFAULT_ANNOTATION_COLOR });

const COLOR_KEY = 'xuewen-annotation-color';

/// Load the remembered color. Call once at startup.
export function initAnnotationTools(): void {
  try {
    const saved = localStorage.getItem(COLOR_KEY);
    if (isAnnotationColor(saved)) annotationTools.color = saved;
  } catch {
    /* no localStorage — the default color still applies */
  }
}

export function setToolColor(c: AnnotationColor): void {
  annotationTools.color = c;
  try {
    localStorage.setItem(COLOR_KEY, c);
  } catch {
    /* no localStorage — the choice still applies, only persistence is lost */
  }
}

export function setActiveTool(k: AnnotationKind | null): void {
  annotationTools.active = k;
}

/// Toolbar click: arm the tool, or disarm it if it was already armed.
export function toggleTool(k: AnnotationKind): void {
  annotationTools.active = annotationTools.active === k ? null : k;
}

/// Leaving the reader (closing the last tab, entering a modal flow) must not
/// leave a tool armed over a document the user can no longer see.
export function disarmTools(): void {
  annotationTools.active = null;
}
