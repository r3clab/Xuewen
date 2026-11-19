import { describe, expect, it } from 'vitest';
import { ZoomMode } from '@embedpdf/svelte/stage';
import { formatScale, isActivePreset, ZOOM_PRESETS } from './zoomPresets';

describe('formatScale', () => {
  it('renders whole percents', () => {
    expect(formatScale(1)).toBe('100%');
    expect(formatScale(1.234)).toBe('123%');
    expect(formatScale(0.5)).toBe('50%');
  });
});

describe('ZOOM_PRESETS', () => {
  it('lists the numeric presets ascending, then the fit modes', () => {
    expect(ZOOM_PRESETS.map((p) => p.label)).toEqual([
      '50%', '75%', '100%', '125%', '150%', '200%', 'Fit width', 'Fit page',
    ]);
    expect(ZOOM_PRESETS[6].level).toBe(ZoomMode.FitWidth);
    expect(ZOOM_PRESETS[7].level).toBe(ZoomMode.FitPage);
  });
});

describe('isActivePreset', () => {
  const fixed = (zoomLevel: number) => ({ zoomLevel, zoomMode: 'custom' as const });

  it('matches a fixed zoom to the level at the displayed whole percent', () => {
    const hundred = ZOOM_PRESETS[2];
    expect(isActivePreset(hundred, fixed(1))).toBe(true);
    expect(isActivePreset(hundred, fixed(1.004))).toBe(true); // rounds to 100%
    expect(isActivePreset(hundred, fixed(1.2))).toBe(false);
  });

  it('checks the fit mode in force, and no level it lands on', () => {
    const fitPage = { zoomLevel: 1, zoomMode: ZoomMode.FitPage };
    expect(isActivePreset(ZOOM_PRESETS[7], fitPage)).toBe(true);
    expect(isActivePreset(ZOOM_PRESETS[6], fitPage)).toBe(false);
    expect(isActivePreset(ZOOM_PRESETS[2], fitPage)).toBe(false);
    expect(isActivePreset(ZOOM_PRESETS[7], fixed(1))).toBe(false);
  });

  it('puts every level on the ladder the zoom buttons walk', () => {
    const ladder = [0.5, 2 / 3, 0.75, 1, 1.25, 1.5, 2, 3, 4];
    for (const p of ZOOM_PRESETS) if (typeof p.level === 'number') expect(ladder).toContain(p.level);
  });
});
