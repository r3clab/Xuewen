import type { Reference } from './references';
import type { StructuredReference } from '$shared/api/generated/StructuredReference';

/** Attach index-aligned structured parses to references (non-destructive). */
export function mergeStructured(
  refs: Reference[],
  structured: (StructuredReference | null)[],
): Reference[] {
  return refs.map((r, i) => (i < structured.length ? { ...r, structured: structured[i] } : r));
}
