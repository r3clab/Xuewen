import type { Attachment } from 'svelte/attachments';

const CONTROL = 'button, input, select';

/// `{@attach rovingToolbar}` on a `role="toolbar"`: the WAI toolbar pattern.
/// The toolbar is one Tab stop — the control last used — and ArrowLeft,
/// ArrowRight, Home and End move between its controls. Keys typed into a text
/// field stay with it (the caret needs them), and an open menu inside the
/// toolbar keeps its own navigation.
export const rovingToolbar: Attachment<HTMLElement> = (toolbar) => {
  let current: HTMLElement | null = null;

  const controls = () =>
    Array.from(toolbar.querySelectorAll<HTMLElement>(CONTROL)).filter((el) => !el.closest('[role="menu"]'));
  const reachable = () =>
    controls().filter((el) => !el.matches(':disabled') && getComputedStyle(el).display !== 'none');

  // Re-run whenever controls appear, go or toggle `disabled`, so the Tab stop
  // never lands on a control that can't take focus.
  const sync = () => {
    const live = reachable();
    if (!current || !live.includes(current)) current = live[0] ?? null;
    for (const el of controls()) el.tabIndex = el === current ? 0 : -1;
  };

  const onfocusin = (e: FocusEvent) => {
    if (e.target instanceof HTMLElement && controls().includes(e.target)) {
      current = e.target;
      sync();
    }
  };

  const onkeydown = (e: KeyboardEvent) => {
    const from = e.target;
    if (!(from instanceof HTMLElement) || from instanceof HTMLInputElement || from.closest('[role="menu"]')) return;
    const live = reachable();
    const i = live.indexOf(from);
    if (i === -1) return;
    const next = {
      ArrowRight: live[(i + 1) % live.length],
      ArrowLeft: live[(i - 1 + live.length) % live.length],
      Home: live[0],
      End: live[live.length - 1],
    }[e.key];
    if (!next) return;
    e.preventDefault();
    next.focus();
  };

  sync();
  const observer = new MutationObserver(sync);
  observer.observe(toolbar, { subtree: true, childList: true, attributes: true, attributeFilter: ['disabled'] });
  toolbar.addEventListener('focusin', onfocusin);
  toolbar.addEventListener('keydown', onkeydown);
  return () => {
    observer.disconnect();
    toolbar.removeEventListener('focusin', onfocusin);
    toolbar.removeEventListener('keydown', onkeydown);
  };
};
