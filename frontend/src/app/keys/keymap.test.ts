import { describe, expect, it } from 'vitest';
import {
  chordKeyLabel,
  exactChord,
  isLeaderRoot,
  LEADER_CHORDS,
  matchingChords,
  SHORTCUT_GROUPS,
  SINGLE_KEYS,
} from './keymap';

describe('leader chords', () => {
  it('every chord is at least two keys and starts at a leader root', () => {
    for (const c of LEADER_CHORDS) {
      expect(c.keys.length, chordKeyLabel(c.keys)).toBeGreaterThan(1);
      expect(isLeaderRoot(c.keys[0])).toBe(true);
    }
  });

  it('no chord root collides with a single-key binding', () => {
    const roots = new Set(LEADER_CHORDS.map((c) => c.keys[0]));
    for (const b of SINGLE_KEYS) expect(roots.has(b.key), b.key).toBe(false);
  });

  it('matchingChords keeps only strict prefixes', () => {
    expect(matchingChords([' ']).map((c) => c.keys.join(''))).toContain(' f');
    expect(matchingChords([' ', 'f'])).toEqual([]);
    expect(matchingChords(['q'])).toEqual([]);
  });

  it('exactChord matches a complete sequence only', () => {
    expect(exactChord([' ', 'f'])?.label).toBe('Find paper…');
    expect(exactChord([' '])).toBeUndefined();
    expect(exactChord([' ', 'f', 'f'])).toBeUndefined();
  });

  it('spells Space out and leaves other keys alone', () => {
    expect(chordKeyLabel([' ', 'f'])).toBe('Space f');
    expect(chordKeyLabel(['g', 'g'])).toBe('g g');
  });
});

describe('help overlay', () => {
  it('lists every chord in the Anywhere group', () => {
    const anywhere = SHORTCUT_GROUPS.find((g) => g.title === 'Anywhere')!;
    for (const c of LEADER_CHORDS) {
      expect(anywhere.items).toContainEqual({ keys: chordKeyLabel(c.keys), label: c.label });
    }
  });
});
