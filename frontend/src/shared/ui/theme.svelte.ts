import { dur } from './motion';
import { readLocal, writeLocal } from '$shared/browser/persist';

export type ThemeMode = 'light' | 'dark' | 'system';
export const theme = $state<{ mode: ThemeMode }>({ mode: 'system' });

const THEME_KEY = 'xuewen-theme';

const darkQuery = (): MediaQueryList => window.matchMedia('(prefers-color-scheme: dark)');

// Resolve a mode to whether the dark class should be applied. 'system' tracks
// the live OS preference; explicit modes ignore it.
function resolvesDark(mode: ThemeMode): boolean {
  return mode === 'dark' || (mode === 'system' && darkQuery().matches);
}
function applyTheme(): void {
  document.documentElement.classList.toggle('dark', resolvesDark(theme.mode));
}
export function initTheme(): void {
  const saved = readLocal(THEME_KEY);
  theme.mode = saved === 'light' || saved === 'dark' || saved === 'system' ? saved : 'system';
  applyTheme();
  // Keep 'system' in sync when the OS preference changes at runtime.
  darkQuery().addEventListener('change', () => {
    if (theme.mode === 'system') applyTheme();
  });
}
const THEME_CYCLE: ThemeMode[] = ['light', 'dark', 'system'];
/// Non-mutating peek at what toggleTheme would switch to — the one source
/// for "click for <next>" tooltips, so they can't drift from the cycle.
export function nextTheme(): ThemeMode {
  return THEME_CYCLE[(THEME_CYCLE.indexOf(theme.mode) + 1) % THEME_CYCLE.length];
}
export function toggleTheme(): void {
  theme.mode = nextTheme();
  writeLocal(THEME_KEY, theme.mode);
  // Crossfade the whole page where the View Transitions API exists; fall
  // back to an instant swap (also under reduced motion / tests via dur).
  const doc = document as Document & { startViewTransition?: (cb: () => void) => unknown };
  if (doc.startViewTransition && dur(1) > 0) {
    doc.startViewTransition(() => applyTheme());
  } else {
    applyTheme();
  }
}

/// Dark-mode page appearance for the PDF reader: dim eases the glare of the
/// white page, invert flips it dark. Applied via dark-scoped CSS (app.css)
/// on the raster layers only, so the preference is inert in light mode.
export type PdfAppearance = 'normal' | 'dim' | 'invert';
export const pdfAppearance = $state<{ mode: PdfAppearance }>({ mode: 'normal' });
const PDF_APPEARANCE_KEY = 'xuewen-pdf-appearance';
const PDF_APPEARANCE_CYCLE: PdfAppearance[] = ['normal', 'dim', 'invert'];

export function initPdfAppearance(): void {
  const saved = readLocal(PDF_APPEARANCE_KEY);
  if (saved === 'normal' || saved === 'dim' || saved === 'invert') pdfAppearance.mode = saved;
}

/// Non-mutating peek, same contract as nextTheme().
export function nextPdfAppearance(): PdfAppearance {
  const idx = PDF_APPEARANCE_CYCLE.indexOf(pdfAppearance.mode);
  return PDF_APPEARANCE_CYCLE[(idx + 1) % PDF_APPEARANCE_CYCLE.length];
}

export function cyclePdfAppearance(): void {
  pdfAppearance.mode = nextPdfAppearance();
  writeLocal(PDF_APPEARANCE_KEY, pdfAppearance.mode);
}
