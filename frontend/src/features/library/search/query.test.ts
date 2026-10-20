import { describe, expect, it } from 'vitest';
import {
  hasSearchTerms,
  parseQuery,
  setFieldQualifiers,
  setQualifier,
  setStarredQualifier,
} from './query';

// Mirrors the Rust fixtures in src/search/query.rs — keep the two in sync.
describe('parseQuery', () => {
  it('treats bare words as free text', () => {
    const p = parseQuery('attention is all you need');
    expect(p.text).toBe('attention is all you need');
    expect(p.tag).toBeNull();
    expect(p.authors).toEqual([]);
    expect(p.starred).toBe(false);
  });

  it('parses a single tag', () => {
    const p = parseQuery('tag:nlp');
    expect(p.tag).toBe('nlp');
    expect(p.text).toBe('');
  });

  it('parses quoted values and all filter keys', () => {
    const p = parseQuery('tag:"deep learning" project:Thesis is:starred status:needs-review');
    expect(p.tag).toBe('deep learning');
    expect(p.project).toBe('Thesis');
    expect(p.starred).toBe(true);
    expect(p.status).toBe('needs_review');
    expect(p.text).toBe('');
  });

  it('unions in: tokens', () => {
    const p = parseQuery('in:title in:abstract transformers');
    expect(p.fields).toEqual(['title', 'abstract']);
    expect(p.text).toBe('transformers');
  });

  it('scopes to annotation notes with in:notes', () => {
    const p = parseQuery('in:notes attention');
    expect(p.fields).toEqual(['notes']);
    expect(p.text).toBe('attention');
  });

  it('collects repeated author terms', () => {
    const p = parseQuery('author:smith author:"ada lovelace" attention');
    expect(p.authors).toEqual(['smith', 'ada lovelace']);
    expect(p.text).toBe('attention');
  });

  it('keeps the last repeated filter key', () => {
    expect(parseQuery('tag:a tag:b').tag).toBe('b');
  });

  it('degrades unknown keys and values to free text', () => {
    const p = parseQuery('foo:bar is:open in:everything tag:');
    expect(p.text).toBe('foo:bar is:open in:everything tag:');
    expect(p.tag).toBeNull();
    expect(p.fields).toBeNull();
    expect(p.starred).toBe(false);
  });

  it('passes quoted phrases through', () => {
    expect(parseQuery('"exact phrase" more').text).toBe('"exact phrase" more');
  });

  it('is case-insensitive on keys, preserves value case', () => {
    const p = parseQuery('TAG:NLP In:Title');
    expect(p.tag).toBe('NLP');
    expect(p.fields).toEqual(['title']);
  });

  it('parses the empty string', () => {
    const p = parseQuery('');
    expect(p.text).toBe('');
    expect(p.tag).toBeNull();
  });

  it('runs an unclosed quote to end of string', () => {
    expect(parseQuery('tag:"unclosed').tag).toBe('unclosed');
  });

  it('splits only on ASCII whitespace — NBSP stays inside a token', () => {
    // Pasted-from-PDF text carries NBSP; the Rust parser keeps it in the
    // value, so the mirror must not let JS `\s` split there.
    const p = parseQuery('tag:deep\u00A0learning');
    expect(p.tag).toBe('deep\u00A0learning');
    expect(p.text).toBe('');
  });
});

describe('setQualifier', () => {
  it('appends a new qualifier', () => {
    expect(setQualifier('attention', 'tag', 'nlp')).toBe('attention tag:nlp');
  });
  it('quotes values with spaces', () => {
    expect(setQualifier('', 'project', 'my thesis')).toBe('project:"my thesis"');
  });
  it('replaces an existing qualifier (removing duplicates)', () => {
    expect(setQualifier('tag:a x tag:b', 'tag', 'c')).toBe('x tag:c');
  });
  it('removes a qualifier with null', () => {
    expect(setQualifier('tag:nlp attention', 'tag', null)).toBe('attention');
  });
  it('leaves free text alone', () => {
    expect(setQualifier('foo:bar', 'tag', null)).toBe('foo:bar');
  });
});

describe('setStarredQualifier', () => {
  it('adds and removes is:starred', () => {
    expect(setStarredQualifier('x', true)).toBe('x is:starred');
    expect(setStarredQualifier('x is:starred', false)).toBe('x');
  });
  it('does not touch other is: values', () => {
    expect(setStarredQualifier('is:open', false)).toBe('is:open');
  });
});

describe('setFieldQualifiers', () => {
  it('writes one in: token per field', () => {
    expect(setFieldQualifiers('x', ['title', 'body'])).toBe('x in:title in:body');
  });
  it('every field means no tokens', () => {
    const all = ['title', 'authors', 'abstract', 'body', 'notes'] as const;
    expect(setFieldQualifiers('in:title x', [...all])).toBe('x');
  });
  it('null clears tokens', () => {
    expect(setFieldQualifiers('in:title in:body x', null)).toBe('x');
  });
});

describe('hasSearchTerms', () => {
  it('true for text or authors, false for qualifiers only', () => {
    expect(hasSearchTerms(parseQuery('attention'))).toBe(true);
    expect(hasSearchTerms(parseQuery('author:smith'))).toBe(true);
    expect(hasSearchTerms(parseQuery('tag:nlp is:starred'))).toBe(false);
    expect(hasSearchTerms(parseQuery(''))).toBe(false);
  });
});
