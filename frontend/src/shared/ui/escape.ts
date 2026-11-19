/// The surfaces Esc dismisses, innermost on top. Every open dialog, menu,
/// popover and in-place editor holds a layer while it is mounted; the global
/// keymap (app/keys/shortcuts.ts) hands each Esc to the top one and goes no
/// further, so one keystroke never dismisses two things.

import type { Attachment } from 'svelte/attachments';

export interface LayerOptions {
  /// A modal layer makes everything behind it ignore the keymap.
  modal?: boolean;
  /// A layer that only owns Esc some of the time: a hidden tab's find bar
  /// stays mounted, an editor with nothing typed has nothing to cancel.
  active?: () => boolean;
}

interface Layer {
  readonly close: () => void;
  readonly modal: boolean;
  readonly active: () => boolean;
}

const layers: Layer[] = [];

/// Register a layer; the returned function removes it.
export function pushLayer(close: () => void, { modal = false, active = () => true }: LayerOptions = {}): () => void {
  const layer: Layer = { close, modal, active };
  layers.push(layer);
  return () => {
    const i = layers.indexOf(layer);
    if (i !== -1) layers.splice(i, 1);
  };
}

/// `{@attach escapeLayer(close)}`: the element is a layer while it is mounted.
export function escapeLayer(close: () => void, options?: LayerOptions): Attachment {
  return () => pushLayer(close, options);
}

/// Give an Esc keydown to the innermost active layer. False — the event left
/// untouched — when no layer wants it.
export function dismissTopLayer(e: KeyboardEvent): boolean {
  if (e.key !== 'Escape') return false;
  const top = layers.findLast((l) => l.active());
  if (!top) return false;
  e.preventDefault();
  top.close();
  return true;
}

export function modalOpen(): boolean {
  return layers.some((l) => l.modal);
}
