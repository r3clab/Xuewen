import { describe, expect, it } from 'vitest';
import { errorMessage } from './errors';

describe('errorMessage', () => {
  it("uses an Error's message", () => {
    expect(errorMessage(new Error('boom'), 'fallback')).toBe('boom');
  });

  it('falls back for anything else, and for an empty message', () => {
    expect(errorMessage({ reason: 1 }, 'fallback')).toBe('fallback');
    expect(errorMessage('text', 'fallback')).toBe('fallback');
    expect(errorMessage(new Error(''), 'fallback')).toBe('fallback');
  });
});
