import { request, requestVoid } from '$shared/api/http';
import type { ImportResult } from '$shared/api/generated/ImportResult';

export async function importPaper(file: File): Promise<ImportResult> {
  const body = new FormData();
  body.append('file', file, file.name);
  return request('/api/papers', 'import failed', { method: 'POST', body });
}

export async function importUrl(input: string): Promise<ImportResult> {
  return request('/api/import', 'import failed', { method: 'POST', json: { input } });
}

export async function setProxyCookie(cookie: string): Promise<void> {
  return requestVoid('/api/settings/proxy-cookie', 'save cookie failed', {
    method: 'PUT',
    json: { cookie },
  });
}

export async function clearProxyCookie(): Promise<void> {
  return requestVoid('/api/settings/proxy-cookie', 'clear cookie failed', { method: 'DELETE' });
}
