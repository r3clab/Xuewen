/// TS mirror of the canonical Rust parser (src/search/query.rs). The two
/// share fixture cases — change one, change both.

import type { PaperStatus } from '$shared/api/generated/PaperStatus';

export type FieldKey = 'title' | 'authors' | 'abstract' | 'body' | 'notes';
export const FIELD_KEYS: readonly FieldKey[] = ['title', 'authors', 'abstract', 'body', 'notes'];

/// Fields the vector tier actually holds. Mirrors `FieldSel::semantic_applicable`
/// in src/search/fts.rs: `authors` and `notes` are never chunked or embedded, so a
/// query scoped to them alone has nothing for semantic search to match.
export const SEMANTIC_FIELD_KEYS: readonly FieldKey[] = ['title', 'abstract', 'body'];

export interface ParsedQuery {
  text: string;
  authors: string[];
  fields: FieldKey[] | null;
  tag: string | null;
  project: string | null;
  starred: boolean;
  status: PaperStatus | null;
}

interface Token {
  raw: string;
  key: string | null;
  value: string;
}

/// Token separators, exactly Rust's `u8::is_ascii_whitespace` (space, \t,
/// \n, \f, \r — no \v). JS `\s` would also split on NBSP and other Unicode
/// whitespace the canonical parser keeps inside a token, desyncing the pill
/// bar from what the server actually filters on.
const ASCII_WHITESPACE = /[ \t\n\f\r]/;

function tokenize(raw: string): Token[] {
  const out: Token[] = [];
  let i = 0;
  while (i < raw.length) {
    while (i < raw.length && ASCII_WHITESPACE.test(raw[i])) i += 1;
    if (i >= raw.length) break;
    const start = i;
    let inQuotes = false;
    while (i < raw.length && (inQuotes || !ASCII_WHITESPACE.test(raw[i]))) {
      if (raw[i] === '"') inQuotes = !inQuotes;
      i += 1;
    }
    out.push(classify(raw.slice(start, i)));
  }
  return out;
}

/// Split `key:value` (key = ASCII letters, value non-empty after unquoting).
function classify(tok: string): Token {
  const colon = tok.indexOf(':');
  if (colon > 0) {
    const key = tok.slice(0, colon);
    const value = tok.slice(colon + 1).replace(/^"+|"+$/g, '');
    if (/^[A-Za-z]+$/.test(key) && value !== '') {
      return { raw: tok, key: key.toLowerCase(), value };
    }
  }
  return { raw: tok, key: null, value: '' };
}

export function parseQuery(raw: string): ParsedQuery {
  const q: ParsedQuery = {
    text: '',
    authors: [],
    fields: null,
    tag: null,
    project: null,
    starred: false,
    status: null,
  };
  const fields = new Set<FieldKey>();
  const text: string[] = [];
  for (const t of tokenize(raw)) {
    switch (t.key) {
      case 'tag':
        q.tag = t.value;
        break;
      case 'project':
        q.project = t.value;
        break;
      case 'author':
        q.authors.push(t.value);
        break;
      case 'is':
        if (t.value.toLowerCase() === 'starred') q.starred = true;
        else text.push(t.raw);
        break;
      case 'status': {
        const v = t.value.toLowerCase().replace(/-/g, '_');
        if (v === 'resolved' || v === 'needs_review') q.status = v;
        else text.push(t.raw);
        break;
      }
      case 'in': {
        const v = t.value.toLowerCase() as FieldKey;
        if ((FIELD_KEYS as readonly string[]).includes(v)) fields.add(v);
        else text.push(t.raw);
        break;
      }
      default:
        text.push(t.raw);
    }
  }
  if (fields.size > 0) q.fields = FIELD_KEYS.filter((f) => fields.has(f));
  q.text = text.join(' ');
  return q;
}

/// A query needs the search engines (vs. a plain filtered list) when it has
/// free text or author-scoped terms.
export function hasSearchTerms(p: ParsedQuery): boolean {
  return p.text.trim() !== '' || p.authors.length > 0;
}

function quote(value: string): string {
  return /\s/.test(value) ? `"${value.replace(/"/g, '')}"` : value;
}

/// Rebuild the query from surviving tokens plus an optional appended one.
/// Normalizes whitespace to single spaces — acceptable for pill-click edits.
function rebuild(kept: Token[], append: string | null): string {
  const parts = kept.map((t) => t.raw);
  if (append !== null) parts.push(append);
  return parts.join(' ');
}

export function setQualifier(
  raw: string,
  key: 'tag' | 'project' | 'status',
  value: string | null,
): string {
  const kept = tokenize(raw).filter((t) => t.key !== key);
  return rebuild(kept, value === null ? null : `${key}:${quote(value)}`);
}

export function setStarredQualifier(raw: string, on: boolean): string {
  const kept = tokenize(raw).filter(
    (t) => !(t.key === 'is' && t.value.toLowerCase() === 'starred'),
  );
  return rebuild(kept, on ? 'is:starred' : null);
}

export function setFieldQualifiers(raw: string, fields: FieldKey[] | null): string {
  const kept = tokenize(raw).filter(
    (t) => !(t.key === 'in' && (FIELD_KEYS as readonly string[]).includes(t.value.toLowerCase())),
  );
  const all = fields === null || fields.length === FIELD_KEYS.length;
  const tokens = all ? [] : FIELD_KEYS.filter((f) => fields!.includes(f)).map((f) => `in:${f}`);
  return rebuild(kept, tokens.length ? tokens.join(' ') : null);
}
