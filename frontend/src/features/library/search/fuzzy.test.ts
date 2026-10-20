import { describe, expect, it } from 'vitest';
import { fuzzyScore } from './fuzzy';

describe('fuzzyScore', () => {
  it('matches subsequences case-insensitively', () => {
    expect(fuzzyScore('aiayn', 'Attention Is All You Need')).not.toBeNull();
    expect(fuzzyScore('xyz', 'Attention Is All You Need')).toBeNull();
  });

  it('empty query matches everything with score 0', () => {
    expect(fuzzyScore('', 'anything')).toBe(0);
  });

  it('prefers consecutive and prefix matches', () => {
    const consecutive = fuzzyScore('atten', 'Attention Is All You Need')!;
    const scattered = fuzzyScore('atn', 'Attention Is All You Need')!;
    expect(consecutive).toBeGreaterThan(scattered);
    const prefix = fuzzyScore('lora', 'LoRA: Low-Rank Adaptation')!;
    const inner = fuzzyScore('lora', 'Exploring LoRA Variants')!;
    expect(prefix).toBeGreaterThan(inner);
  });
});
