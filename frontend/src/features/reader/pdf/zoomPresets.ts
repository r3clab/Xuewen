import { ZoomMode } from '@embedpdf/plugin-zoom';

/// One entry of the toolbar's zoom menu. Numeric levels are engine scale
/// factors (1 = 100%); fit modes delegate the math to the zoom plugin.
export interface ZoomPreset {
  label: string;
  level: ZoomMode | number;
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

/// A numeric preset is active when it matches the live zoom at the same
/// whole-percent granularity the UI displays. Fit modes are never marked —
/// the live value is numeric even when a fit mode produced it.
export function isActivePreset(preset: ZoomPreset, currentZoomLevel: number): boolean {
  return (
    typeof preset.level === 'number' &&
    Math.round(preset.level * 100) === Math.round(currentZoomLevel * 100)
  );
}
