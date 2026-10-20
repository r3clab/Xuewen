import { dropAnnotations } from '$features/reader/annotations/store.svelte';
import { disarmTools } from '$features/reader/annotations/state.svelte';
import { ApiError } from '$shared/api/http';
import { getPaper } from '$features/library/api';
import { readLocalJson, writeLocal } from '$shared/browser/persist';
import { dropReaderState } from '$features/reader/state.svelte';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';

export interface Tab {
  id: string;
  title: string;
  /// The paper's manual "known as" name, when it has one. The strip labels the
  /// tab with it — "RVSpec" beats a 15-word title truncated to nothing at
  /// max-w-52 — while `title` stays the full title for the tooltip and for
  /// everything else reading off the tab (the annotation export filename, the
  /// reader toolbar).
  name: string | null;
}
/// The content pane's tab strip. `activeId === null` means the permanent
/// "Library" home tab is active (shows the library table); a string means
/// that PDF tab is active. Tabs persist while home is active.
export const viewer = $state<{ tabs: Tab[]; activeId: string | null; zen: boolean }>({
  tabs: [],
  activeId: null,
  zen: false,
});

/// The browsing highlight for the Library list (moved by j/k). Distinct from viewer.activeId: the highlight is the list cursor; opening a paper reads it.
export const selection = $state<{ id: string | null }>({ id: null });

export function selectPaper(id: string | null): void {
  selection.id = id;
}

/// Activate the Library home tab (keeps PDF tabs open). Leaving the reader
/// always leaves zen too — zen without a PDF is a blank screen.
export function goHome(): void {
  viewer.activeId = null;
  viewer.zen = false;
  saveTabs();
}

/// The header's search box and review count work over a PDF, but what they
/// change only shows on the Library view.
export function revealLibrary(): void {
  if (viewer.activeId !== null) goHome();
}

/// Zen requires an active PDF tab; toggling from home is a no-op.
export function toggleZen(): void {
  viewer.zen = viewer.activeId !== null && !viewer.zen;
}

export function openTab(p: PaperSummary): void {
  if (!viewer.tabs.some((t) => t.id === p.id)) {
    viewer.tabs.push({ id: p.id, title: p.title ?? p.cite_key ?? p.id, name: p.name });
  }
  viewer.activeId = p.id;
  selection.id = p.id;
  saveTabs();
}

export function closeTab(id: string): void {
  const idx = viewer.tabs.findIndex((t) => t.id === id);
  if (idx === -1) return;
  viewer.tabs.splice(idx, 1);
  dropReaderState(id);
  // The rows stay on the server; reopening reloads them. Keeping the cache
  // would only mean a closed paper's marks linger in memory.
  dropAnnotations(id);
  if (viewer.activeId === id) {
    viewer.activeId = viewer.tabs[Math.max(0, idx - 1)]?.id ?? null;
  }
  if (viewer.tabs.length === 0) {
    viewer.zen = false;
    // Leaving the reader must not leave a tool armed: the next opened tab
    // would silently inherit a highlighter with no visible cause.
    disarmTools();
  }
  saveTabs();
}

/// Switch the reader to an already-open tab (the tab strip's click target).
export function activateTab(id: string | null): void {
  viewer.activeId = id;
  saveTabs();
}

export function retitleTab(id: string, title: string | null): void {
  const tab = viewer.tabs.find((t) => t.id === id);
  if (tab) tab.title = title ?? tab.title;
}

export function renameTab(id: string, name: string | null): void {
  const tab = viewer.tabs.find((t) => t.id === id);
  if (tab) {
    tab.name = name;
    saveTabs();
  }
}

const TABS_KEY = 'xuewen-tabs';

function saveTabs(): void {
  writeLocal(
    TABS_KEY,
    JSON.stringify({
      tabs: viewer.tabs.map((t) => ({ id: t.id, title: t.title, name: t.name })),
      activeId: viewer.activeId,
    }),
  );
}

/// Restore the remembered tab set (and active tab) at startup, then prune
/// ids the server no longer knows — a paper deleted or purged since the last
/// session must not resurrect as a dead tab. Restore is immediate (titles
/// come from storage); the validation round-trip only removes losers after.
export async function initTabs(): Promise<void> {
  const parsed = readLocalJson(TABS_KEY);
  if (!parsed || typeof parsed !== 'object') return; // absent or corrupted — start with no tabs
  const { tabs: rawTabs, activeId } = parsed as { tabs?: unknown; activeId?: unknown };
  const tabs = Array.isArray(rawTabs)
    ? (rawTabs as unknown[]).flatMap((t): Tab[] => {
        const r = t as Partial<Tab> | null;
        if (!r || typeof r.id !== 'string' || typeof r.title !== 'string') return [];
        // `name` post-dates this storage key, so a tab written by an older
        // build simply has none — normalize rather than reject, or every
        // remembered tab would vanish on the upgrade.
        return [{ id: r.id, title: r.title, name: typeof r.name === 'string' ? r.name : null }];
      })
    : [];
  if (tabs.length === 0) return;
  viewer.tabs = tabs;
  viewer.activeId =
    typeof activeId === 'string' && tabs.some((t) => t.id === activeId) ? activeId : null;
  const alive = new Set(
    (
      await Promise.all(
        tabs.map(async (t) => {
          try {
            await getPaper(t.id);
            return t.id;
          } catch (e) {
            // Prune only when the server positively said the paper is gone.
            // A transient failure — backend restarting, network down, a 500 —
            // must not erase (and re-save!) the remembered workspace.
            return e instanceof ApiError && (e.status === 404 || e.status === 410) ? null : t.id;
          }
        }),
      )
    ).filter((id): id is string => id !== null),
  );
  if (alive.size !== tabs.length) {
    viewer.tabs = viewer.tabs.filter((t) => alive.has(t.id));
    if (viewer.activeId !== null && !alive.has(viewer.activeId)) viewer.activeId = null;
    saveTabs();
  }
}
