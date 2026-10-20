export interface ToastAction {
  label: string;
  run: () => void;
}

export interface Toast {
  id: number;
  kind: 'success' | 'error' | 'info';
  message: string;
  action?: ToastAction;
}

export const toasts = $state<{ items: Toast[] }>({ items: [] });

let nextId = 1;

/// Show a transient toast. Returns the id (for programmatic dismissal).
/// timeoutMs 0 = sticky. Toasts are additive feedback — persistent errors
/// must also stay inline where they occur. An `action` renders as a button
/// (e.g. Undo) that runs once and dismisses the toast.
export function toast(
  kind: Toast['kind'],
  message: string,
  timeoutMs = 3500,
  action?: ToastAction,
): number {
  const id = nextId++;
  toasts.items.push({ id, kind, message, action });
  if (timeoutMs > 0) setTimeout(() => dismissToast(id), timeoutMs);
  return id;
}

export function dismissToast(id: number): void {
  const idx = toasts.items.findIndex((t) => t.id === id);
  if (idx !== -1) toasts.items.splice(idx, 1);
}
