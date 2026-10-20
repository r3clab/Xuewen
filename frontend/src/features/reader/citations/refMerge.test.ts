import { describe, expect, it } from 'vitest';
import { mergeStructured } from './refMerge';
import type { Reference } from './references';
import type { StructuredReference } from '$shared/api/generated/StructuredReference';

const ref = (index: number): Reference => ({ index, destPageIndex: 0, destY: 0, rawText: `raw ${index}` });
const s: StructuredReference = {
  authors: ['D. Kingma'], title: 'Adam', venue: 'ICLR', year: 2015,
  doi: null, arxiv_id: '1412.6980', url: null,
};

describe('mergeStructured', () => {
  it('attaches structured entries by index and leaves others untouched', () => {
    const out = mergeStructured([ref(0), ref(1)], [s, null]);
    expect(out[0].structured).toEqual(s);
    expect(out[1].structured).toBeNull();
    expect(out[0].rawText).toBe('raw 0');
  });

  it('tolerates a shorter structured array', () => {
    const out = mergeStructured([ref(0), ref(1)], [s]);
    expect(out[1].structured).toBeUndefined();
  });
});
