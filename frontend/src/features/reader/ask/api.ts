import { request, requestRaw, requestVoid } from '$shared/api/http';
import type { ChatModels } from '$shared/api/generated/ChatModels';
import type { ChatTurnRow } from '$shared/api/generated/ChatTurnRow';
import type { CodeAttachment } from '$shared/api/generated/CodeAttachment';

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

/** POST a chat message; returns the raw streaming Response (SSE body) —
 *  the only endpoint whose caller needs the stream, so it alone hands the
 *  Response back instead of parsed JSON. */
export async function postChatMessage(
  paperId: string,
  body: { model_id: string | null; message: string },
  signal: AbortSignal,
): Promise<Response> {
  const res = await requestRaw(
    `/api/papers/${encodeURIComponent(paperId)}/chat`,
    'chat request failed',
    { method: 'POST', json: body, signal },
  );
  if (!res.body) throw new Error('chat request failed: empty response body');
  return res;
}

// --- code (attach-a-repo for the agent; Task 6 on the backend) ---

export async function getPaperCode(id: string): Promise<CodeAttachment> {
  return request(`/api/papers/${encodeURIComponent(id)}/code`, 'loading the code status failed');
}

export async function setPaperCode(id: string, repoUrl: string): Promise<CodeAttachment> {
  return request(`/api/papers/${encodeURIComponent(id)}/code`, 'attaching the repo failed', {
    method: 'PUT',
    json: { repo_url: repoUrl },
  });
}

export async function removePaperCode(id: string): Promise<void> {
  return requestVoid(`/api/papers/${encodeURIComponent(id)}/code`, 'removing the repo failed', {
    method: 'DELETE',
  });
}
