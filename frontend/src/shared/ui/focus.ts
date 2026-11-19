import type { Attachment } from 'svelte/attachments';

/// Focus an element, selecting an input's text so a typed key replaces it.
function focusAndSelect(el: HTMLElement): void {
  el.focus();
  if (el instanceof HTMLInputElement) el.select();
}

/// `{@attach focusOnMount}`: focus the element as it mounts (input text
/// selected), for surfaces outside a `<dialog>` — inside one the `autofocus`
/// attribute does this.
export const focusOnMount: Attachment<HTMLElement> = focusAndSelect;

/// `{@attach returnFocus}` on a popup that takes focus (WAI menu pattern):
/// hand focus back to wherever it was when the popup closes.
export const returnFocus: Attachment = () => {
  const previous = document.activeElement;
  return () => {
    if (previous instanceof HTMLElement) previous.focus();
  };
};

/// A focus destination code elsewhere can send focus to — the keymap's `/`
/// to the search box, ⌘F to a find bar. The element registers itself with
/// `{@attach target.attach}`; a caller that has just made it render calls
/// `flushSync()` before `focus()` so the element is there to receive it.
export class FocusTarget {
  #el: HTMLElement | null = null;

  readonly attach = (el: HTMLElement): (() => void) => {
    this.#el = el;
    return () => {
      if (this.#el === el) this.#el = null;
    };
  };

  /// Focus the element (input text selected). False when nothing is mounted.
  focus(): boolean {
    if (!this.#el) return false;
    focusAndSelect(this.#el);
    return true;
  }
}
