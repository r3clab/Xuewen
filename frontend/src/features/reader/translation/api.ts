import { request } from '$shared/api/http';
import type { TranslateBody } from '$shared/api/generated/TranslateBody';
import type { TranslationDto } from '$shared/api/generated/TranslationDto';

export async function translateText(body: TranslateBody): Promise<TranslationDto> {
  return request('/api/translate', 'translate failed', { method: 'POST', json: body });
}
