import { request } from '$shared/api/http';
import type { Candidate } from '$shared/api/generated/Candidate';
import type { IdentifyBody } from '$shared/api/generated/IdentifyBody';
import type { PaperDetail } from '$shared/api/generated/PaperDetail';

export async function identifySearch(q: string): Promise<Candidate[]> {
  return request(`/api/identify/search?q=${encodeURIComponent(q)}`, 'search failed');
}

export async function identifyPaper(id: string, body: IdentifyBody): Promise<PaperDetail> {
  return request(`/api/papers/${encodeURIComponent(id)}/identify`, 'identify failed', {
    method: 'POST',
    json: body,
  });
}
