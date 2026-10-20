<script lang="ts">
  import { columnResize } from './columnResize';

  let {
    label,
    width,
    min,
    max,
    edge = 'right',
    onLiveResize,
    onCommit,
    onAutoFit,
  }: {
    label: string;
    width: number;
    min: number;
    max: () => number;
    edge?: 'left' | 'right';
    onLiveResize: (px: number) => void;
    onCommit: (px: number) => void;
    onAutoFit: () => void;
  } = $props();
</script>

<!-- Focusable window-splitter (WAI separator pattern): drag or ArrowLeft/
     ArrowRight resize the column, double-click auto-fits it. aria-valuemax
     is deliberately omitted — the ceiling is container-dependent and
     computing it would mean a clientWidth read on every render. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -- the separator IS the
     interactive splitter; keyboard resize needs it focusable. -->
<div
  role="separator"
  aria-orientation="vertical"
  aria-label={`Resize ${label} column`}
  aria-valuenow={Math.round(width)}
  aria-valuemin={min}
  tabindex="0"
  use:columnResize={{ width, min, max, edge, onResize: onLiveResize, onCommit, onAutoFit }}
  class={`absolute inset-y-0 z-10 w-2 cursor-col-resize touch-none select-none rounded-full hover:bg-amber-700/30 focus-visible:bg-amber-700/50 focus-visible:outline-none dark:hover:bg-amber-500/30 dark:focus-visible:bg-amber-500/50 ${
    edge === 'left' ? '-left-1' : '-right-1'
  }`}
></div>
