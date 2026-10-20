import { describe, expect, test } from 'vitest';
import { isPrefixMatch } from './tagTree';

describe('isPrefixMatch', () => {
  test('prefix match includes children', () => {
    expect(isPrefixMatch('security', 'security')).toBe(true);
    expect(isPrefixMatch('security/fuzzing', 'security')).toBe(true);
    expect(isPrefixMatch('ml/llm', 'security')).toBe(false);
  });
});
