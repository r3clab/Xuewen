import { clearAnnotationSelection, redoAnnotation, undoAnnotation } from '$features/reader/annotations/commands';
import { identifyState } from '$features/identify/state.svelte';
import { isLeaderRoot, openSelected, SINGLE_KEYS } from './keymap';
import { advanceLeader, cancelLeader, leader } from './leader.svelte';
import { copyPdfSelection, pdfSelectionHasText } from '$features/reader/pdfCopy';
import { openFind } from '$features/reader/state.svelte';
import { viewer } from '$features/reader/tabs/state.svelte';
import { shell } from '$app/shell.svelte';
import { closeDock, dock } from '$features/reader/dock/state.svelte';
import { importState } from '$features/importing/queue.svelte';
import { finder } from '$features/library/search/state.svelte';

export function isEditable(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false;
  return !!(
    t instanceof HTMLInputElement ||
    t instanceof HTMLTextAreaElement ||
    t instanceof HTMLSelectElement ||
    t.isContentEditable
  );
}

function anyModalOpen(): boolean {
  return importState.open || identifyState.open || shell.helpOpen || finder.open;
}

/// True when the browser has a real text selection of its own. Every surface
/// outside the PDF page area — library rows, the details dock, the Ask
/// transcript, popovers, the reader's own toolbar — is ordinary DOM text where
/// native copy already works, so ⌘C must stand aside there. The page area is
/// the sole exception, and it can never produce a selection to confuse this:
/// its pages are <img>, its overlays are empty <div>s, the Viewport carries
/// `select-none`, and pdfCopy clears any stale selection when a PDF selection
/// begins.
function hasDomSelection(): boolean {
  return (document.getSelection()?.toString() ?? '').trim() !== '';
}

/// Global keymap driver. Single keys live as data in keymap.ts
/// (SINGLE_KEYS), multi-key sequences in LEADER_CHORDS; this dispatches both
/// after the gating below. Modals own their Esc (Modal.svelte stops
/// propagation); everything is inert while a modal — the file picker
/// included — is open or focus is in a text control. Spec deviation:
/// close-tab is `x`, not ⌘W — browsers reserve ⌘W/Ctrl+W for closing the
/// browser tab.
export function handleKeydown(e: KeyboardEvent): void {
  if (anyModalOpen()) return;
  // No chord contains a modifier, so this keystroke belongs to one of the
  // branches below. Abandoning the sequence here is what keeps a stray Space
  // before a ⌘-shortcut from swallowing the *next* key, or completing a chord
  // the user never started.
  if (e.metaKey || e.ctrlKey || e.altKey) cancelLeader();
  // ⌘F finds in the open PDF; on the Library view the browser find is fine.
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'f') {
    if (viewer.activeId) {
      e.preventDefault();
      openFind(viewer.activeId);
    }
    return;
  }
  // Keyboard events from inside a shadow DOM (the PDF viewer) retarget
  // `e.target` to the shadow host, which is never editable — so keys typed
  // into the viewer's find box would leak to these app shortcuts. Check the
  // real deepest target from the composed path instead.
  const realTarget = e.composedPath()[0] ?? e.target;
  // ⌘C copies the reader's text selection. The reader is the one place in the
  // app where the browser cannot do this itself — its pages are <img> and its
  // selection overlay is empty <div>s, so the document selection there is
  // always collapsed and no `copy` event is ever dispatched (see pdfCopy.ts).
  // Everywhere else IS real DOM text, so this branch stands aside — no
  // preventDefault, no call — on a live DOM selection, in a text control, on
  // the Library view, or with nothing selected in the PDF, and native copy
  // proceeds untouched. ⌥/⇧ are excluded so ⌘⌥C and ⌘⇧C (DevTools inspect)
  // still reach the browser. copyPdfSelection() must not be awaited or
  // deferred: it writes the clipboard synchronously, while this keystroke's
  // user gesture still authorizes the write.
  if ((e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'c') {
    if (viewer.activeId && !isEditable(realTarget) && !hasDomSelection() && pdfSelectionHasText()) {
      e.preventDefault();
      copyPdfSelection();
    }
    return;
  }
  // ⌘Z / ⇧⌘Z take back the last mark, through the same 'annotations' history
  // topic the toolbar's undo buttons drive — so the keys and the buttons can
  // never disagree, and neither can reach whatever else may one day share the
  // history plugin's timeline. Like ⌘C above this stands aside rather than
  // guessing: in a text control the browser's own undo is what the typist
  // wants, and on the library view or with an empty stack the command reports
  // it did nothing, so the keystroke goes on untouched. ⌥ is excluded so
  // ⌘⌥Z stays available to the browser.
  if ((e.metaKey || e.ctrlKey) && !e.altKey && e.key.toLowerCase() === 'z') {
    if (isEditable(realTarget)) return;
    if (e.shiftKey ? redoAnnotation() : undoAnnotation()) e.preventDefault();
    return;
  }
  if (e.key === 'Escape') {
    // First rung: a half-typed chord is the most transient thing on screen,
    // and Helix's own Esc abandons a pending sequence before anything else.
    if (cancelLeader()) return;
    // Before the dock and zen rungs: a selected mark is the most local thing
    // on screen, and it is the one piece of reader state with no other way
    // out — clicking elsewhere on the page is the alternative. The command's
    // own answer is the rung's condition, so a deselect that couldn't happen
    // falls through to the dock rather than swallowing the key.
    else if (clearAnnotationSelection()) {
      // Deselected. Esc stops here rather than also closing the dock.
    } else if (dock.open && viewer.activeId !== null) closeDock();
    else if (viewer.zen) viewer.zen = false;
    return;
  }
  if (isEditable(realTarget)) return;
  if (e.metaKey || e.ctrlKey || e.altKey) return;
  // Match letters case-insensitively so Caps Lock or a held Shift doesn't
  // dead-key a shortcut (`Z`/`X` would otherwise miss `z`/`x`). Named keys
  // (Enter, ArrowUp, …) are longer than one char and keep their exact spelling.
  const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  // Leader sequences run ahead of the single-key table and swallow their
  // keys: `Space` then `j` must not also move the library selection. A key
  // that starts no chord, mid-sequence, cancels rather than falling through
  // — a mistyped second key is a mistake, not a new command. Space is left
  // to the browser where it would activate a focused control, the same guard
  // Enter applies below, so a focused row or star button still works.
  if (leader.pending.length > 0 || isLeaderRoot(key)) {
    const overControl =
      leader.pending.length === 0 &&
      realTarget instanceof HTMLElement &&
      realTarget.closest('button, a, summary');
    if (!overControl) {
      e.preventDefault();
      advanceLeader(key);
      return;
    }
  }
  const binding = SINGLE_KEYS.find((b) => b.key === key);
  if (binding) {
    if (binding.when && !binding.when()) return;
    if (binding.preventDefault) e.preventDefault();
    binding.run();
    return;
  }
  if (key === 'Enter') {
    // Enter on a focused control activates that control — it must not also
    // open the selected paper. Checked against the composed-path target, so
    // a button inside the viewer's shadow DOM counts too (raw e.target would
    // be its never-matching host).
    if (realTarget instanceof HTMLElement && realTarget.closest('button, a, summary')) return;
    openSelected();
  }
}
