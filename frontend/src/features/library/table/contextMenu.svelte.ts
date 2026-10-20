import type { PaperSummary } from '$shared/api/generated/PaperSummary';

/// Shared state for the single app-level paper context menu. Only one menu is
/// ever open, so a lone `$state` record drives the root-mounted
/// `PaperContextMenu.svelte` — rows just call `openContextMenu` on right-click
/// rather than each mounting their own menu.
export const contextMenu = $state<{
  open: boolean;
  x: number;
  y: number;
  paper: PaperSummary | null;
}>({ open: false, x: 0, y: 0, paper: null });

export function openContextMenu(e: MouseEvent, paper: PaperSummary): void {
  e.preventDefault();
  contextMenu.open = true;
  contextMenu.x = e.clientX;
  contextMenu.y = e.clientY;
  contextMenu.paper = paper;
}

export function closeContextMenu(): void {
  contextMenu.open = false;
  contextMenu.paper = null;
}
