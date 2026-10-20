import { request } from '$shared/api/http';
import type { StructuredReference } from '$shared/api/generated/StructuredReference';

/** Parse extracted reference strings via the backend LLM service. Returns
 *  null on ANY failure (503 = [ai.citations] unconfigured, network error,
 *  unexpected shape) — the popover then just keeps showing raw text. */
export async function parseCitations(
  paperId: string,
  references: string[],
): Promise<(StructuredReference | null)[] | null> {
  try {
    const j = await request<{ references?: (StructuredReference | null)[] }>(
      `/api/papers/${encodeURIComponent(paperId)}/citations`,
      'parsing citations failed',
      { method: 'POST', json: { references } },
    );
    return Array.isArray(j.references) ? j.references : null;
  } catch {
    return null;
  }
}
