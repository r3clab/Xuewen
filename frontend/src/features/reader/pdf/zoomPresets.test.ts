import { describe, expect, it } from 'vitest';
import { ZoomMode } from '@embedpdf/plugin-zoom';
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
  it('matches a numeric preset to the displayed whole percent', () => {
    const hundred = ZOOM_PRESETS[2];
    expect(isActivePreset(hundred, 1)).toBe(true);
    expect(isActivePreset(hundred, 1.004)).toBe(true); // rounds to 100%
    expect(isActivePreset(hundred, 1.2)).toBe(false);
  });

  it('never marks fit modes active', () => {
    expect(isActivePreset(ZOOM_PRESETS[6], 1)).toBe(false);
    expect(isActivePreset(ZOOM_PRESETS[7], 1)).toBe(false);
  });
});
