import { fireEvent, render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

const stage = { goToDestination: vi.fn() };
const bibliographyPage = { ref: { objectNumber: 12 } };

vi.mock('@embedpdf/svelte/stage', () => ({ useStage: () => stage }));
vi.mock('@embedpdf/svelte/runtime', () => ({
  usePageList: () => ({ current: [{ ref: { objectNumber: 11 } }, bibliographyPage] }),
}));

import CitationLayer from './CitationLayer.svelte';
import type { CitationData } from './references';

const data: CitationData = {
  references: [{ index: 0, destPageIndex: 1, destY: 300, rawText: '[1] A. Author. A title. 2020.' }],
  markers: [{ pageIndex: 0, x: 100, y: 200, width: 12, height: 10, refIndex: 0 }],
};
const where = { kind: 'xyz', page: bibliographyPage.ref, x: null, y: 276, zoom: null };

function renderLayer() {
  render(CitationLayer, { pageIndex: 0, pageWidthPt: 612, pageHeightPt: 792, data, matches: new Map() });
  return screen.getByRole('link', { name: /Citation reference 1/ });
}

describe('CitationLayer', () => {
  it('jumps to the bibliography entry on click', async () => {
    stage.goToDestination.mockClear();
    await userEvent.click(renderLayer());
    expect(stage.goToDestination).toHaveBeenCalledWith(where, { behavior: 'smooth' });
  });

  it('takes Enter for itself, so the keymap does not also open the selected paper', async () => {
    stage.goToDestination.mockClear();
    const marker = renderLayer();
    expect(await fireEvent.keyDown(marker, { key: 'Enter' })).toBe(false);
    expect(stage.goToDestination).toHaveBeenCalledWith(where, { behavior: 'smooth' });
    expect(await fireEvent.keyDown(marker, { key: 'a' })).toBe(true);
  });
});
