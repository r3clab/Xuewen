import { deleteChatThread, getChatModels, getChatThread, postChatMessage } from './api';
import { readLocal, writeLocal } from '$shared/browser/persist';
import { readSse } from '$shared/api/sse';
import type { ChatModelInfo } from '$shared/api/generated/ChatModelInfo';
import type { ChatTurnRow } from '$shared/api/generated/ChatTurnRow';
import type { ToolCall } from '$shared/api/generated/ToolCall';

/// The floating paper-chat. `pending` is the user message awaiting a reply,
/// `streaming` the assistant text accumulating under it; both fold into
/// `messages` only when the server confirms the exchange was stored.
export const chat = $state<{
  available: boolean;
  models: ChatModelInfo[];
  modelId: string | null;
  paperId: string | null;
  messages: ChatTurnRow[];
  pending: string | null;
  streaming: string | null;
  streamTools: ToolCall[];
  busy: boolean;
  error: string | null;
  draft: string;
}>({
  available: false,
  models: [],
  modelId: null,
  paperId: null,
  messages: [],
  pending: null,
  streaming: null,
  streamTools: [],
  busy: false,
  error: null,
  draft: '',
});

const CHAT_MODEL_KEY = 'xuewen-chat-model';

// Bumped whenever the thread identity changes; in-flight streams from a
// superseded thread must not write into the current one (same pattern as
// identifySession in features/identify/actions.ts).
let session = 0;
let aborter: AbortController | null = null;
// Client-assigned ids for optimistic turns: negative and descending, so they
// never collide with server row ids or with each other in keyed eaches.
let localId = -1;

export async function loadChatModels(): Promise<void> {
  try {
    const body = await getChatModels();
    chat.models = body.models;
    chat.available = body.available && body.models.length > 0;
    const saved = readLocal(CHAT_MODEL_KEY);
    chat.modelId = chat.models.some((m) => m.id === saved)
      ? saved
      : (chat.models[0]?.id ?? null);
  } catch {
    chat.available = false;
  }
}

export function setChatModel(id: string): void {
  chat.modelId = id;
  writeLocal(CHAT_MODEL_KEY, id);
}

export async function loadThread(paperId: string): Promise<void> {
  if (chat.paperId === paperId) return;
  const my = ++session;
  aborter?.abort();
  chat.paperId = paperId;
  chat.messages = [];
  chat.pending = null;
  chat.streaming = null;
  chat.streamTools = [];
  chat.busy = false;
  chat.error = null;
  try {
    const rows = await getChatThread(paperId);
    if (my === session) chat.messages = rows;
  } catch {
    if (my === session) {
      chat.paperId = null; // un-latch so reopening the panel retries the load
      chat.error = 'Could not load this conversation. Close and reopen the chat to retry.';
    }
  }
}

export async function sendChatMessage(): Promise<void> {
  const text = chat.draft.trim();
  if (!text || chat.busy || !chat.paperId || chat.modelId === null) return;
  const my = session;
  chat.pending = text;
  chat.draft = '';
  chat.busy = true;
  chat.error = null;
  chat.streaming = '';
  chat.streamTools = [];
  const myAborter = new AbortController();
  aborter = myAborter;
  let failure: string | null = null;
  let completed = false;
  try {
    const resp = await postChatMessage(
      chat.paperId,
      { model_id: chat.modelId, message: text },
      myAborter.signal,
    );
    await readSse(resp.body!, (e) => {
      if (my !== session) return;
      if (e.event === 'delta') {
        chat.streaming = (chat.streaming ?? '') + (JSON.parse(e.data).text ?? '');
      } else if (e.event === 'tool') {
        const t = JSON.parse(e.data) as { name?: string; detail?: string };
        chat.streamTools.push({ name: String(t.name ?? ''), detail: String(t.detail ?? '') });
      } else if (e.event === 'error') {
        failure = String(JSON.parse(e.data).message ?? 'unknown error');
      } else if (e.event === 'done') {
        completed = true;
        const label = chat.models.find((m) => m.id === chat.modelId)?.label ?? null;
        const doneId: unknown = JSON.parse(e.data).id;
        chat.messages.push({
          id: localId--,
          role: 'user',
          content: text,
          model: null,
          created_at: '',
          tools: null,
        });
        chat.messages.push({
          id: doneId == null ? localId-- : Number(doneId),
          role: 'assistant',
          content: chat.streaming ?? '',
          model: label,
          created_at: '',
          tools: chat.streamTools.length ? [...chat.streamTools] : null,
        });
        chat.pending = null;
        chat.streaming = null;
        chat.streamTools = [];
      }
    });
    if (my !== session) return;
    if (failure) throw new Error(failure);
    if (!completed) throw new Error('the connection closed before the reply finished');
  } catch (err) {
    // Once the exchange folded on `done`, a trailing read rejection (e.g. a
    // Stop clicked in the post-done window) must not repopulate the draft or
    // report a spurious error over a successful exchange.
    if (my !== session || completed) return;
    const aborted = err instanceof DOMException && err.name === 'AbortError';
    chat.pending = null;
    chat.streaming = null;
    chat.streamTools = [];
    chat.draft = text; // give the message back for editing or resend
    chat.error = aborted
      ? null
      : `The model request failed: ${(err as Error).message} Send again to retry.`;
  } finally {
    if (my === session) chat.busy = false;
    if (aborter === myAborter) aborter = null; // never null a newer send's controller
  }
}

export function stopChatStream(): void {
  aborter?.abort();
}

export async function clearChatThread(): Promise<void> {
  if (!chat.paperId) return;
  try {
    await deleteChatThread(chat.paperId);
    chat.messages = [];
    chat.error = null;
  } catch {
    chat.error = 'Could not clear this conversation. Try again.';
  }
}
