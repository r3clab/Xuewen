import { describe, expect, it } from 'vitest';
import { authorLine, refLinks, titleCase } from './refFormat';
import type { StructuredReference } from '$shared/api/generated/StructuredReference';

describe('titleCase', () => {
  it('capitalizes words except small ones', () => {
    expect(titleCase('mapping global dynamics of benchmark creation and saturation in artificial intelligence'))
      .toBe('Mapping Global Dynamics of Benchmark Creation and Saturation in Artificial Intelligence');
  });
  it('always capitalizes the first and last word, and after a colon', () => {
    expect(titleCase('the road to autonomy')).toBe('The Road to Autonomy');
    expect(titleCase('adam: a method for stochastic optimization')).toBe('Adam: A Method for Stochastic Optimization');
    expect(titleCase('what are we looking for')).toBe('What Are We Looking For');
  });
  it('leaves acronyms, identifiers, and mixed-case words untouched', () => {
    expect(titleCase('PGFUZZ: policy-guided fuzzing for robotic vehicles'))
      .toBe('PGFUZZ: Policy-Guided Fuzzing for Robotic Vehicles');
    expect(titleCase('training GPT-4 and eBPF probes on iOS'))
      .toBe('Training GPT-4 and eBPF Probes on iOS');
    // Chicago-style: "with" stays lowercase (title-case package behavior).
    expect(titleCase('deep learning with differential privacy')).toBe('Deep Learning with Differential Privacy');
  });
  it('capitalizes hyphen parts except small ones', () => {
    expect(titleCase('state-of-the-art symbolic execution')).toBe('State-of-the-Art Symbolic Execution');
  });
});

const base: StructuredReference = {
  authors: [], title: null, venue: null, year: null, doi: null, arxiv_id: null, url: null,
};

describe('authorLine', () => {
  it('shows one or two names verbatim', () => {
    expect(authorLine(['A. One'])).toBe('A. One');
    expect(authorLine(['A. One', 'B. Two'])).toBe('A. One, B. Two');
  });
  it('collapses three or more to first and last', () => {
    expect(authorLine(['A. One', 'B. Two', 'C. Three'])).toBe('A. One, …, C. Three');
    expect(authorLine(['A', 'B', 'C', 'D'])).toBe('A, …, D');
  });
  it('empty list gives empty string', () => {
    expect(authorLine([])).toBe('');
  });
});

describe('refLinks', () => {
  it('prefers DOI, then arXiv, dedupes by href, caps at 2', () => {
    const links = refLinks({ ...base, doi: '10.1/x', arxiv_id: '1412.6980', url: 'https://a.b/c' });
    expect(links).toEqual([
      { label: 'doi.org', href: 'https://doi.org/10.1/x' },
      { label: 'arXiv', href: 'https://arxiv.org/abs/1412.6980' },
    ]);
  });
  it('falls back to the raw externalUrl when unparsed', () => {
    expect(refLinks(null, 'https://doi.org/10.1/x')).toEqual([
      { label: 'doi.org', href: 'https://doi.org/10.1/x' },
    ]);
  });
  it('dedupes structured DOI against the same externalUrl', () => {
    const links = refLinks({ ...base, doi: '10.1/x' }, 'https://doi.org/10.1/x');
    expect(links).toHaveLength(1);
  });
  it('drops non-http(s) and unparseable hrefs (javascript:, data:, garbage)', () => {
    expect(refLinks({ ...base, url: 'javascript:alert(1)' })).toEqual([]);
    expect(refLinks(null, 'data:text/html,<script>alert(1)</script>')).toEqual([]);
    expect(refLinks({ ...base, url: 'not a url' })).toEqual([]);
    expect(refLinks({ ...base, url: 'https://ok.example/x' })).toEqual([{ label: 'ok.example', href: 'https://ok.example/x' }]);
  });
});
