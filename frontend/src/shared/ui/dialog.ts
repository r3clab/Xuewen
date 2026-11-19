import type { Attachment } from 'svelte/attachments';
import { pushLayer } from './escape';

/// `{@attach modalDialog(close)}` on a `<dialog>`: shown modally for as long
/// as it is mounted. The browser makes the page behind it inert, contains
/// focus, and returns focus to the opener when it closes. Esc reaches it
/// through the layer stack like every other surface (the browser's own
/// `cancel` folds into the same path), and a click on the backdrop closes it.
export function modalDialog(close: () => void): Attachment<HTMLDialogElement> {
  return (dialog) => {
    dialog.showModal();
    const pop = pushLayer(close, { modal: true });
    const oncancel = (e: Event) => {
      e.preventDefault();
      close();
    };
    const onclick = (e: MouseEvent) => {
      if (e.target === dialog) close();
    };
    dialog.addEventListener('cancel', oncancel);
    dialog.addEventListener('click', onclick);
    return () => {
      pop();
      dialog.removeEventListener('cancel', oncancel);
      dialog.removeEventListener('click', onclick);
      dialog.close();
    };
  };
}
