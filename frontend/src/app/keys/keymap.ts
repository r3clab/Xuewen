import { tick } from 'svelte';
import { annotationSelectionActive, deleteSelectedAnnotations } from '$features/reader/annotations/commands';
import { chat } from '$features/reader/ask/chat.svelte';
import { library } from '$features/library/state.svelte';
import { toggleAnnotationsPanel } from '$features/reader/state.svelte';
import { closeTab, openTab, selectPaper, selection, toggleZen, viewer } from '$features/reader/tabs/state.svelte';
import { shell } from '$app/shell.svelte';
import { toggleDock } from '$features/reader/dock/state.svelte';
import { finder } from '$features/library/search/state.svelte';

function moveSelection(delta: number): void {
  const papers = library.papers;
  if (papers.length === 0) return;
  const idx = papers.findIndex((p) => p.id === selection.id);
  const next = idx === -1 ? (delta > 0 ? 0 : papers.length - 1) : Math.min(papers.length - 1, Math.max(0, idx + delta));
  selectPaper(papers[next].id);
}

/// Enter's action — the binding itself stays bespoke in shortcuts.ts because
/// its focused-control guard needs the event's composed-path target.
export function openSelected(): void {
  const p = library.papers.find((x) => x.id === selection.id);
  if (p) openTab(p);
}

/// Zen unmounts the top bar the search box lives in, so `/` leaves zen first
/// and focuses once it is back.
function focusSearch(): void {
  viewer.zen = false;
  void tick().then(() => {
    document.querySelector<HTMLInputElement>('[data-search-input]')?.focus();
  });
}

export interface KeyBinding {
  key: string;
  label: string;
  /// Gate: the key is inert (dispatch skipped entirely) when this is false.
  when?: () => boolean;
  /// Only keys the browser has its own use for (quick-find) suppress it.
  preventDefault?: boolean;
  run: () => void;
}

/// The two keys a reader reaches for after clicking a mark, sharing one
/// definition so they can't drift apart. Gated on a mark being selected, so
/// they stay inert everywhere else in the app; preventDefault because Backspace
/// still means "go back" in some browsers, and a stray navigation would take
/// the whole session with it.
const deleteMark = {
  label: 'Delete selected annotation',
  when: annotationSelectionActive,
  preventDefault: true,
  run: deleteSelectedAnnotations,
};

/// The single-key map as data — the one source shared by the dispatcher
/// (shortcuts.ts) and the `?` help overlay (SHORTCUT_GROUPS below), so a
/// binding can't drift between behavior and display. Multi-key sequences
/// live in LEADER_CHORDS below; modifier chords (⌘F/⌘C/⌘Z), Esc, and Enter
/// stay bespoke in shortcuts.ts — each needs the event itself — and appear
/// below as static display rows only.
export const SINGLE_KEYS: readonly KeyBinding[] = [
  { key: '/', label: 'Search library', preventDefault: true, run: focusSearch },
  { key: '?', label: 'Keyboard shortcuts', preventDefault: true, run: () => (shell.helpOpen = true) },
  { key: 'c', label: 'Paper panel · ask', when: () => chat.available, run: () => toggleDock('ask') },
  { key: 'i', label: 'Paper panel', run: () => toggleDock('record') },
  {
    key: 'a',
    label: 'Annotations panel',
    // Reader-only: the panel lives inside the PDF view, and on the library
    // this would open a panel nobody can see.
    when: () => viewer.activeId !== null,
    run: toggleAnnotationsPanel,
  },
  { key: 'z', label: 'Zen mode', run: toggleZen },
  { key: 'x', label: 'Close tab', when: () => viewer.activeId !== null, run: () => closeTab(viewer.activeId!) },
  { key: 'j', label: 'Next paper', run: () => moveSelection(1) },
  { key: 'k', label: 'Previous paper', run: () => moveSelection(-1) },
  { key: 'Delete', ...deleteMark },
  { key: 'Backspace', ...deleteMark },
];

/// A multi-key sequence, Helix-style: `Space f` is `[' ', 'f']`. The whole
/// sequence lives in one entry so a second leader (or a deeper chord) is a
/// new row here rather than new branches in the dispatcher.
export interface ChordBinding {
  /// At least two keys — a single key belongs in SINGLE_KEYS instead.
  keys: readonly string[];
  label: string;
  /// Gate: the chord still consumes its keys, it just does nothing.
  when?: () => boolean;
  run: () => void;
}

/// The leader map. `Space` is the only root today; the dispatcher reads the
/// roots off this table, so adding `g` (or `Space b`) needs no change there.
export const LEADER_CHORDS: readonly ChordBinding[] = [
  { keys: [' ', 'f'], label: 'Find paper…', run: () => (finder.open = true) },
];

/// Does `key` start some chord? The dispatcher's test for "claim this key".
export function isLeaderRoot(key: string): boolean {
  return LEADER_CHORDS.some((c) => c.keys[0] === key);
}

/// Chords that `pending` is a strict prefix of — what may still follow.
export function matchingChords(pending: readonly string[]): readonly ChordBinding[] {
  return LEADER_CHORDS.filter(
    (c) => c.keys.length > pending.length && pending.every((k, i) => c.keys[i] === k),
  );
}

/// The chord `pending` completes exactly, if any.
export function exactChord(pending: readonly string[]): ChordBinding | undefined {
  return LEADER_CHORDS.find(
    (c) => c.keys.length === pending.length && pending.every((k, i) => c.keys[i] === k),
  );
}

/// A chord as the help overlay and the hint spell it: `[' ', 'f']` → "Space f".
export function chordKeyLabel(keys: readonly string[]): string {
  return keys.map((k) => (k === ' ' ? 'Space' : k)).join(' ');
}

export interface ShortcutItem {
  keys: string;
  label: string;
}

/// A single-key binding's display row, pulled from the table above.
function fromKey(key: string): ShortcutItem {
  const b = SINGLE_KEYS.find((x) => x.key === key)!;
  return { keys: b.key, label: b.label };
}

/// Rendered by the `?` help overlay. Single-key rows derive from SINGLE_KEYS
/// (j/k fold into one row); chord/Esc/Enter rows are the static display of
/// the bespoke handlers in shortcuts.ts.
export const SHORTCUT_GROUPS: ReadonlyArray<{ title: string; items: ShortcutItem[] }> = [
  {
    title: 'Library',
    items: [
      fromKey('/'),
      // j/k are two bindings but one display row — the explicit pairing rule.
      { keys: `${fromKey('j').keys} / ${fromKey('k').keys}`, label: 'Next / previous paper' },
      { keys: 'Enter', label: 'Open selected paper' },
    ],
  },
  {
    title: 'Reader',
    items: [
      fromKey('i'),
      fromKey('c'),
      fromKey('a'),
      fromKey('z'),
      fromKey('x'),
      { keys: '⌘F', label: 'Find in PDF' },
      { keys: '⌘C', label: 'Copy selected text' },
      // Two bindings, one display row — the j/k pairing rule again.
      { keys: `${fromKey('Delete').keys} / ${fromKey('Backspace').keys}`, label: fromKey('Delete').label },
      { keys: '⌘Z', label: 'Undo annotation' },
      { keys: '⇧⌘Z', label: 'Redo annotation' },
    ],
  },
  {
    title: 'Anywhere',
    items: [
      // Derived, so a new chord shows up here with no edit.
      ...LEADER_CHORDS.map((c) => ({ keys: chordKeyLabel(c.keys), label: c.label })),
      fromKey('?'),
      { keys: 'Esc', label: 'Close panel · exit zen' },
    ],
  },
];
