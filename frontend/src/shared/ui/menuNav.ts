/// Roving focus for a `role="menu"` popover (WAI menu pattern), wired once
/// in ContextMenuShell for every cursor-anchored menu.

export function menuItems(menuEl: HTMLElement | null): HTMLElement[] {
  return menuEl ? Array.from(menuEl.querySelectorAll<HTMLElement>('[role="menuitem"]')) : [];
}

/// ArrowUp/ArrowDown cycle through the menu's items with wrap-around;
/// Home/End jump to the ends. Every other key is left alone.
export function menuNavKeydown(menuEl: HTMLElement | null, e: KeyboardEvent): void {
  const list = menuItems(menuEl);
  if (list.length === 0) return;
  const idx = list.indexOf(document.activeElement as HTMLElement);
  const wrap = (n: number) => (n + list.length) % list.length;
  if (e.key === 'ArrowDown') {
    e.preventDefault();
    list[wrap(idx + 1)].focus();
  } else if (e.key === 'ArrowUp') {
    e.preventDefault();
    list[idx === -1 ? list.length - 1 : wrap(idx - 1)].focus();
  } else if (e.key === 'Home') {
    e.preventDefault();
    list[0].focus();
  } else if (e.key === 'End') {
    e.preventDefault();
    list[list.length - 1].focus();
  }
}
