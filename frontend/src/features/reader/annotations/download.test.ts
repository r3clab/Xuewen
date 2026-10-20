import { describe, expect, it } from 'vitest';
import { annotatedFilename, sanitizeFilename } from './download';

describe('annotatedFilename', () => {
  it('puts the suffix before the extension', () => {
    expect(annotatedFilename('Attention Is All You Need.pdf')).toBe(
      'Attention Is All You Need (annotated).pdf',
    );
    expect(annotatedFilename('no-extension')).toBe('no-extension (annotated).pdf');
  });

  it('does not stack the suffix when exporting an export', () => {
    expect(annotatedFilename('Paper (annotated).pdf')).toBe('Paper (annotated).pdf');
  });

  it('matches the extension case-insensitively', () => {
    expect(annotatedFilename('Paper.PDF')).toBe('Paper (annotated).pdf');
  });

  it('falls back for a blank name', () => {
    expect(annotatedFilename('')).toBe('paper (annotated).pdf');
    expect(annotatedFilename('   ')).toBe('paper (annotated).pdf');
    // A name that is nothing BUT an extension.
    expect(annotatedFilename('.pdf')).toBe('(annotated).pdf');
  });
});

describe('sanitizeFilename', () => {
  it('replaces path separators and reserved punctuation', () => {
    expect(sanitizeFilename('a/b\\c:d*e?f"g<h>i|j')).toBe('a b c d e f g h i j');
  });

  it('strips control characters', () => {
    expect(sanitizeFilename('a\x00b\x1fc')).toBe('a b c');
  });

  it('collapses runs of whitespace and trims dots and spaces', () => {
    expect(sanitizeFilename('  a   b  ')).toBe('a b');
    // Windows drops a trailing dot silently, so we drop it visibly.
    expect(sanitizeFilename('report...')).toBe('report');
    expect(sanitizeFilename('...report')).toBe('report');
  });

  it('never returns an empty name', () => {
    expect(sanitizeFilename('')).toBe('paper');
    expect(sanitizeFilename('///')).toBe('paper');
    expect(sanitizeFilename('...')).toBe('paper');
  });

  it('leaves ordinary names and non-ASCII alone', () => {
    expect(sanitizeFilename('學問 2026')).toBe('學問 2026');
  });
});
