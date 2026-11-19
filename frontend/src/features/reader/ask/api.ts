import { request, requestRaw, requestVoid } from '$shared/api/http';
import type { ChatModels } from '$shared/api/generated/ChatModels';
import type { ChatRequest } from '$shared/api/generated/ChatRequest';
import type { ChatTurnRow } from '$shared/api/generated/ChatTurnRow';
import type { CodeAttachment } from '$shared/api/generated/CodeAttachment';
import type { CodeBody } from '$shared/api/generated/CodeBody';

// --- chat (HTTP layer only; session/stream bookkeeping lives in chat.svelte.ts) ---

export async function getChatModels(): Promise<ChatModels> {
  return request('/api/chat/models', 'chat models failed');
}

export async function getChatThread(paperId: string): Promise<ChatTurnRow[]> {
  return request(`/api/papers/${encodeURIComponent(paperId)}/chat`, 'chat thread failed');
}

export async function deleteChatThread(paperId: string): Promise<void> {
  return requestVoid(`/api/papers/${encodeURIComponent(paperId)}/chat`, 'clear chat failed', {
    method: 'DELETE',
  });
}

/** POST a chat message; the reply streams back as server-sent events, each
 *  one `ChatSseEvent` (read them with `sseData`). */
export async function postChatMessage(
  paperId: string,
  body: ChatRequest,
  signal: AbortSignal,
): Promise<ReadableStream<Uint8Array>> {
  const res = await requestRaw(
    `/api/papers/${encodeURIComponent(paperId)}/chat`,
    'chat request failed',
    { method: 'POST', json: body, signal },
  );
  if (!res.body) throw new Error('chat request failed: empty response body');
  return res.body;
}

// --- code (a repository attached for the agent to read) ---

export async function getPaperCode(id: string): Promise<CodeAttachment> {
  return request(`/api/papers/${encodeURIComponent(id)}/code`, 'loading the code status failed');
}

export async function setPaperCode(id: string, repoUrl: string): Promise<CodeAttachment> {
  return request(`/api/papers/${encodeURIComponent(id)}/code`, 'attaching the repo failed', {
    method: 'PUT',
    json: { repo_url: repoUrl } satisfies CodeBody,
  });
}

export async function removePaperCode(id: string): Promise<void> {
  return requestVoid(`/api/papers/${encodeURIComponent(id)}/code`, 'removing the repo failed', {
    method: 'DELETE',
  });
}
