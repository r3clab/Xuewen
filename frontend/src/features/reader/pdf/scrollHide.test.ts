import { describe, expect, it } from 'vitest';
import {
  anchorScrollHide,
  INITIAL_SCROLL_HIDE,
  nextScrollHide,
  SCROLL_HIDE_PX,
  TOP_EPS_PX,
  type ScrollHide,
} from './scrollHide';

const run = (start: ScrollHide, ...offsets: number[]): ScrollHide => offsets.reduce(nextScrollHide, start);

describe('nextScrollHide', () => {
  it('adopts the first observed offset instead of reading it as travel', () => {
    const first = nextScrollHide(INITIAL_SCROLL_HIDE, 4000);
    expect(first).toEqual({ hidden: false, anchor: 4000 });
  });

  it('hides once forward travel passes the threshold', () => {
    expect(run(INITIAL_SCROLL_HIDE, 200, 200 + SCROLL_HIDE_PX).hidden).toBe(true);
  });

  it('ignores forward travel below the threshold', () => {
    expect(run(INITIAL_SCROLL_HIDE, 200, 200 + SCROLL_HIDE_PX - 1).hidden).toBe(false);
  });

  it('reveals on scrolling back', () => {
    const hidden = run(INITIAL_SCROLL_HIDE, 500, 500 + SCROLL_HIDE_PX);
    expect(run(hidden, 500).hidden).toBe(false);
  });

  it('re-anchors on a reversal too small to flip', () => {
    const jittered = run(INITIAL_SCROLL_HIDE, 500, 480);
    expect(jittered).toEqual({ hidden: false, anchor: 480 });
    expect(run(jittered, 480 + SCROLL_HIDE_PX).hidden).toBe(true);
  });

  it('does not accumulate a flip across a reversal', () => {
    expect(run(INITIAL_SCROLL_HIDE, 500, 500 + SCROLL_HIDE_PX - 1, 500, 500 + SCROLL_HIDE_PX - 1).hidden).toBe(false);
  });

  it('always shows at the top of the document', () => {
    const hidden = run(INITIAL_SCROLL_HIDE, 800, 800 + SCROLL_HIDE_PX);
    expect(hidden.hidden).toBe(true);
    expect(nextScrollHide(hidden, TOP_EPS_PX).hidden).toBe(false);
  });
});

describe('anchorScrollHide', () => {
  it('moves the anchor without flipping visibility', () => {
    const hidden = run(INITIAL_SCROLL_HIDE, 500, 500 + SCROLL_HIDE_PX);
    const jumped = anchorScrollHide(hidden, 9000);
    expect(jumped).toEqual({ hidden: true, anchor: 9000 });
  });
});
