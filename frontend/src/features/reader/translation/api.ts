import { request } from '$shared/api/http';
import type { TranslateProvider } from '$shared/api/generated/TranslateProvider';
import type { TranslationDto } from '$shared/api/generated/TranslationDto';

export async function translateText(
  text: string,
  opts?: { provider?: TranslateProvider; targetLang?: string },
): Promise<TranslationDto> {
  return request('/api/translate', 'translate failed', {
    method: 'POST',
    json: { text, provider: opts?.provider, target_lang: opts?.targetLang },
  });
}
