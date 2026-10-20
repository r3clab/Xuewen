import { request } from './http';
import type { Settings } from '$shared/api/generated/Settings';
import type { TranslateSettings } from '$shared/api/generated/TranslateSettings';

async function getSettings(): Promise<Settings> {
  return request('/api/settings', 'settings failed');
}

/// UI preferences and server-held state from `/api/settings` — the single
/// loader/store for that endpoint (the import modal re-calls loadSettings
/// after a proxy-cookie change rather than keeping its own copy). Loaded
/// once at startup.
export const appSettings = $state<{
  foldAbstract: boolean;
  translate: TranslateSettings;
  proxyHost: string | null;
  proxyCookieSet: boolean;
  proxyCookieUpdatedAt: string | null;
}>({
  foldAbstract: true,
  translate: { enabled: false },
  proxyHost: null,
  proxyCookieSet: false,
  proxyCookieUpdatedAt: null,
});

export async function loadSettings(): Promise<void> {
  try {
    const s = await getSettings();
    appSettings.foldAbstract = s.fold_abstract;
    appSettings.translate = s.translate ?? { enabled: false };
    appSettings.proxyHost = s.proxy?.host ?? null;
    appSettings.proxyCookieSet = s.proxy_cookie_set;
    appSettings.proxyCookieUpdatedAt = s.proxy_cookie_updated_at;
  } catch (e) {
    console.error(e); // keep the last known values on failure
  }
}
