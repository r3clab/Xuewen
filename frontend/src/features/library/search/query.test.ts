import { describe, expect, it } from 'vitest';
import {
  SEMANTIC_FIELD_KEYS,
  hasSearchTerms,
  parseQuery,
  setFieldQualifiers,
  setQualifier,
  setStarredQualifier,
} from './query';
import cases from './query_cases.json';

// query_cases.json is the grammar's one spec: the canonical Rust parser
// (src/search/query.rs) runs the same cases.
describe('parseQuery', () => {
  it.each(cases.cases)('parses $input', ({ input, parsed }) => {
    expect(parseQuery(input)).toEqual(parsed);
  });
});

describe('SEMANTIC_FIELD_KEYS', () => {
  it('names exactly the fields the vector tier embeds', () => {
    expect([...SEMANTIC_FIELD_KEYS]).toEqual(cases.semantic_fields);
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
