import { translateText } from './api';
import { appSettings } from '$shared/api/settings.svelte';
import type { TranslateProvider } from '$shared/api/generated/TranslateProvider';

/// Shared state for the single app-level translate popover. Only one
/// translation is ever in flight/shown, so a lone `$state` record drives the
/// root-mounted `TranslatePopover.svelte` (Task 6) the same way `contextMenu`
/// drives `PaperContextMenu.svelte`.
export const translateBox = $state<{
  open: boolean;
  x: number;
  y: number;
  source: string;
  translation: string;
  sourceLang: string | null;
  provider: TranslateProvider | null;
  loading: boolean;
  error: string | null;
}>({
  open: false,
  x: 0,
  y: 0,
  source: '',
  translation: '',
  sourceLang: null,
  provider: null,
  loading: false,
  error: null,
});

/// Selection-translate trigger. Config-only: `[translate].trigger` in
/// xuewen.toml is the single source of truth (the old in-app 譯 toggle and
/// its localStorage state are gone).
export function translateTrigger(): 'auto' | 'manual' {
  return appSettings.translate.trigger === 'manual' ? 'manual' : 'auto';
}

// Guards against a slower, superseded request clobbering a newer one's
// result once both resolve out of order.
let seq = 0;

export async function requestTranslate(text: string, at: { x: number; y: number }, provider?: TranslateProvider): Promise<void> {
  const my = ++seq;
  translateBox.open = true;
  translateBox.x = at.x;
  translateBox.y = at.y;
  translateBox.source = text;
  translateBox.translation = '';
  translateBox.sourceLang = null;
  translateBox.provider = provider ?? appSettings.translate.default_provider ?? null;
  translateBox.loading = true;
  translateBox.error = null;
  try {
    const r = await translateText(text, {
      provider,
      targetLang: appSettings.translate.target_lang,
    });
    if (my !== seq) return; // superseded by a newer request
    translateBox.translation = r.translation;
    translateBox.sourceLang = r.source_lang;
    translateBox.provider = r.provider;
  } catch (e) {
    if (my === seq) translateBox.error = (e as Error).message;
  } finally {
    if (my === seq) translateBox.loading = false;
  }
}

export function closeTranslate(): void {
  seq++; // invalidate any in-flight request
  translateBox.open = false;
  translateBox.translation = '';
  translateBox.source = '';
  translateBox.sourceLang = null;
  translateBox.error = null;
  translateBox.loading = false;
  translateBox.x = 0;
  translateBox.y = 0;
  translateBox.provider = null;
}
