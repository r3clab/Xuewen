import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { LEADER_CHORDS, type ChordBinding } from './keymap';
import { advanceLeader, cancelLeader, leader, leaderContinuations, LEADER_TIMEOUT_MS } from './leader.svelte';
import { finder } from '$features/library/search/state.svelte';

beforeEach(() => {
  vi.useFakeTimers();
  cancelLeader();
  finder.open = false;
});

afterEach(() => {
  vi.useRealTimers();
});

describe('advanceLeader', () => {
  it('holds a root key and reports it consumed', () => {
    expect(advanceLeader(' ')).toBe(true);
    expect(leader.pending).toEqual([' ']);
    expect(leaderContinuations().map((c) => c.label)).toEqual(LEADER_CHORDS.map((c) => c.label));
  });

  it('runs the chord and clears the sequence on the final key', () => {
    advanceLeader(' ');
    expect(advanceLeader('f')).toBe(true);
    expect(finder.open).toBe(true);
    expect(leader.pending).toEqual([]);
  });

  it('swallows a gated chord without running it', () => {
    const gated = { keys: [' ', 'q'], label: 'gated', when: () => false, run: vi.fn() };
    const chords = LEADER_CHORDS as ChordBinding[];
    chords.push(gated);
    try {
      advanceLeader(' ');
      expect(advanceLeader('q')).toBe(true);
      expect(gated.run).not.toHaveBeenCalled();
      expect(leader.pending).toEqual([]);
    } finally {
      chords.pop();
    }
  });

  it('cancels immediately on a key no chord can follow', () => {
    advanceLeader(' ');
    expect(advanceLeader('j')).toBe(true);
    expect(leader.pending).toEqual([]);
    expect(finder.open).toBe(false);
  });

  it('leaves a key alone when nothing was pending and nothing starts', () => {
    expect(advanceLeader('j')).toBe(false);
    expect(leader.pending).toEqual([]);
  });

  it('forgets the sequence after the timeout', () => {
    advanceLeader(' ');
    vi.advanceTimersByTime(LEADER_TIMEOUT_MS + 1);
    expect(leader.pending).toEqual([]);
    // The next `f` is then an ordinary key, not the tail of a stale chord.
    expect(advanceLeader('f')).toBe(false);
    expect(finder.open).toBe(false);
  });

  it('restarts the deadline on each key rather than measuring from the first', () => {
    const deep = { keys: [' ', 'w', 'v'], label: 'deep', run: vi.fn() };
    const chords = LEADER_CHORDS as ChordBinding[];
    chords.push(deep);
    try {
      advanceLeader(' ');
      vi.advanceTimersByTime(LEADER_TIMEOUT_MS - 1);
      advanceLeader('w');
      vi.advanceTimersByTime(LEADER_TIMEOUT_MS - 1);
      expect(leader.pending).toEqual([' ', 'w']);
      advanceLeader('v');
      expect(deep.run).toHaveBeenCalled();
    } finally {
      chords.pop();
    }
  });
});

describe('cancelLeader', () => {
  it('reports whether it had anything to abandon', () => {
    expect(cancelLeader()).toBe(false);
    advanceLeader(' ');
    expect(cancelLeader()).toBe(true);
    expect(leader.pending).toEqual([]);
  });

  it('drops the pending timeout with the sequence', () => {
    advanceLeader(' ');
    cancelLeader();
    advanceLeader('f');
    vi.advanceTimersByTime(LEADER_TIMEOUT_MS + 1);
    expect(leader.pending).toEqual([]);
    expect(finder.open).toBe(false);
  });
});
