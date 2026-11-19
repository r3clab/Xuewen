import type { PaperSummary } from '$shared/api/generated/PaperSummary';

/// Shared state for the single app-level paper context menu, open while it
/// has a `paper`. Only one menu is ever open, so a lone `$state` record
/// drives the root-mounted `PaperContextMenu.svelte` — rows just call
/// `openContextMenu` on right-click rather than each mounting their own menu.
/// `step` walks the two-step delete; every open starts on the action list,
/// a retarget (right-click on a second row while the menu is up) included.
export const contextMenu = $state<{
  x: number;
  y: number;
  paper: PaperSummary | null;
  step: 'actions' | 'confirm-delete' | 'deleting';
}>({ x: 0, y: 0, paper: null, step: 'actions' });

export function openContextMenu(e: MouseEvent, paper: PaperSummary): void {
  e.preventDefault();
  contextMenu.x = e.clientX;
  contextMenu.y = e.clientY;
  contextMenu.paper = paper;
  contextMenu.step = 'actions';
}

export function closeContextMenu(): void {
  contextMenu.paper = null;
}
