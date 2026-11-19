import { describe, expect, it } from 'vitest';
import { searchParams } from './api';

const both = { keyword: true, semantic: true };

describe('searchParams', () => {
  it('sends only q when both engines are selected', () => {
    const p = searchParams('fuzzing', both);
    expect(p.get('q')).toBe('fuzzing');
    expect(p.get('engines')).toBeNull();
    expect([...p.keys()]).toEqual(['q']);
  });

  it('carries the raw query string verbatim (qualifiers parse server-side)', () => {
    const p = searchParams('tag:nlp in:title author:smith attention', both);
    expect(p.get('q')).toBe('tag:nlp in:title author:smith attention');
    expect(p.get('fields')).toBeNull();
    expect(p.get('tag')).toBeNull();
  });

  it('lists only the selected engine', () => {
    const p = searchParams('x', { keyword: true, semantic: false });
    expect(p.get('engines')).toBe('keyword');
  });

  it('keywordOnly overrides the engine selection', () => {
    const p = searchParams('x', both, true);
    expect(p.get('engines')).toBe('keyword');
  });

  it('treats an empty engine selection the same as all (UI enforces at least one)', () => {
    const p = searchParams('x', { keyword: false, semantic: false });
    expect(p.get('engines')).toBeNull();
  });
});
