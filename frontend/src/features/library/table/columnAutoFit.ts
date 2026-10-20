/// Auto-fit width math for the library table. The allocation logic is pure,
/// and the text measurer is injectable, so tests (jsdom has no canvas 2D
/// context and no layout) can supply a deterministic fake.

export type TextMeasurer = (text: string, font: string) => number;

let ctx: CanvasRenderingContext2D | null | undefined;

/// Measures with a lazily created detached canvas. Without a 2D context
/// (jsdom) it falls back to a rough per-character estimate — approximate is
/// fine here, auto-fit is a convenience, not a layout guarantee.
export const canvasMeasurer: TextMeasurer = (text, font) => {
  if (ctx === undefined) {
    try {
      ctx = document.createElement('canvas').getContext('2d');
    } catch {
      ctx = null;
    }
  }
  if (!ctx) return text.length * 7;
  ctx.font = font;
  return ctx.measureText(text).width;
};

export interface AutoFitCell {
  text: string;
  font: string;
}

export interface AutoFitBounds {
  min: number;
  max: number;
  padding: number;
}

/// Width of the widest cell plus padding, clamped to [min, max].
export function autoFitWidth(
  cells: AutoFitCell[],
  bounds: AutoFitBounds,
  measure: TextMeasurer = canvasMeasurer,
): number {
  let widest = 0;
  for (const cell of cells) {
    if (!cell.text) continue;
    widest = Math.max(widest, measure(cell.text, cell.font));
  }
  return Math.round(Math.min(bounds.max, Math.max(bounds.min, widest + bounds.padding)));
}

/// The `font` shorthand serializes inconsistently across engines (empty in
/// Firefox), so compose it from the longhands.
function fontOf(el: Element): string {
  const s = getComputedStyle(el);
  return `${s.fontStyle} ${s.fontWeight} ${s.fontSize} ${s.fontFamily}`.trim();
}

/// Collect every `[data-col="key"]` element under `root` (the header label
/// plus one per row) and fit to the widest. Reading textContent measures the
/// full text even where CSS `.truncate` clips it; reading each cell's
/// computed font keeps serif titles and sans body cells honest without
/// hardcoding Tailwind's font stacks.
export function measureColumnFromDom(
  root: HTMLElement,
  key: string,
  bounds: AutoFitBounds,
  measure: TextMeasurer = canvasMeasurer,
): number {
  const cells = Array.from(root.querySelectorAll<HTMLElement>(`[data-col="${key}"]`)).map((el) => ({
    text: el.textContent?.trim() ?? '',
    font: fontOf(el),
  }));
  return autoFitWidth(cells, bounds, measure);
}

/// Scale a set of natural (content-fit) widths so they sum toward
/// `available` — shrinking when over budget, and expanding into surplus
/// when under, so leftover pane width goes to the content columns instead
/// of all pooling in the flexible Tags column. Every column stays within
/// its own [min, max]; a column pinned at a bound drops out and the rest
/// re-scale (pinning frees a different amount than its proportional share).
/// If even the minimums overflow, returns the minimums (the table degrades
/// to a thin Tags column, not an error).
export function fitToAvailable<K extends string>(
  natural: Record<K, number>,
  bounds: Record<K, { min: number; max: number }>,
  available: number,
): Record<K, number> {
  const keys = Object.keys(natural) as K[];
  const pinned = new Map<K, number>();
  for (;;) {
    const pinnedSum = [...pinned.values()].reduce((s, v) => s + v, 0);
    const free = keys.filter((k) => !pinned.has(k));
    const freeNatural = free.reduce((s, k) => s + natural[k], 0);
    if (free.length === 0 || freeNatural <= 0) break;
    const scale = (available - pinnedSum) / freeNatural;
    let repinned = false;
    for (const k of free) {
      const scaled = natural[k] * scale;
      if (scaled < bounds[k].min) {
        pinned.set(k, bounds[k].min);
        repinned = true;
      } else if (scaled > bounds[k].max) {
        pinned.set(k, bounds[k].max);
        repinned = true;
      }
    }
    if (!repinned) {
      return Object.fromEntries(
        keys.map((k) => [k, pinned.has(k) ? pinned.get(k)! : Math.floor(natural[k] * scale)]),
      ) as Record<K, number>;
    }
  }
  // Every column pinned (or nothing left to scale): bounds are the answer.
  return Object.fromEntries(keys.map((k) => [k, pinned.get(k) ?? bounds[k].min])) as Record<K, number>;
}
