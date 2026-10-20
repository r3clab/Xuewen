/// Guarded localStorage access — the one home for the persist-and-restore
/// guard. Where storage is disabled (Safari with all cookies blocked,
/// Chromium with site data blocked, some webviews), touching `localStorage`
/// throws outright, so every persistence site goes through these: state
/// still applies, only persistence is lost — and an init-time read can never
/// abort app startup.

export function readLocal(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function writeLocal(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* no localStorage — state still applies, only persistence is lost */
  }
}

/// JSON variant for the structured keys (dock, tabs). A corrupted stored
/// value degrades to null, same as a missing key; callers still validate the
/// shape of whatever parses.
export function readLocalJson(key: string): unknown {
  const raw = readLocal(key);
  if (raw === null) return null;
  try {
    return JSON.parse(raw) as unknown;
  } catch {
    return null;
  }
}
