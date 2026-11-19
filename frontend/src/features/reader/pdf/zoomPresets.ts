import { ZoomMode, type ZoomModeValue } from '@embedpdf/svelte/stage';

/// One entry of the toolbar's zoom menu. Numeric levels are the stage's own
/// zoom factors (1 = 100%, the page at its printed size), each a stop on the
/// ladder zoomIn/zoomOut walk; fit modes delegate the math to the stage.
export interface ZoomPreset {
  label: string;
  level: ZoomModeValue | number;
}

export const ZOOM_PRESETS: ZoomPreset[] = [
  { label: '50%', level: 0.5 },
  { label: '75%', level: 0.75 },
  { label: '100%', level: 1 },
  { label: '125%', level: 1.25 },
  { label: '150%', level: 1.5 },
  { label: '200%', level: 2 },
  { label: 'Fit width', level: ZoomMode.FitWidth },
  { label: 'Fit page', level: ZoomMode.FitPage },
];

/// Whole-percent display for the toolbar's scale button ("120%").
export function formatScale(currentZoomLevel: number): string {
  return `${Math.round(currentZoomLevel * 100)}%`;
}

/// The live zoom as the stage reports it: the factor, and the intent behind it.
export interface LiveZoom {
  readonly zoomLevel: number;
  readonly zoomMode: ZoomModeValue | 'custom';
}

/// Which preset the menu checks. A fit mode is checked while it is the
/// intent (it keeps applying as the window resizes), and then no level is,
/// even one the fit happens to land on; a level is checked when a fixed zoom
/// matches it at the whole-percent granularity the UI displays.
export function isActivePreset(preset: ZoomPreset, live: LiveZoom): boolean {
  if (typeof preset.level !== 'number') return live.zoomMode === preset.level;
  return live.zoomMode === 'custom' && Math.round(preset.level * 100) === Math.round(live.zoomLevel * 100);
}
