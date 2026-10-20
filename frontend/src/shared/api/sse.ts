/// Minimal SSE reader for fetch() response bodies (EventSource cannot POST).
/// Calls onEvent once per complete event; a trailing partial event (stream
/// cut mid-message) is dropped — the caller treats a missing `done` as an
/// interrupted reply.
export interface SseEvent {
  event: string;
  data: string;
}

export async function readSse(
  body: ReadableStream<Uint8Array>,
  onEvent: (e: SseEvent) => void,
): Promise<void> {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buf = '';
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    buf += decoder.decode(value, { stream: true });
    let idx: number;
    while ((idx = buf.indexOf('\n\n')) !== -1) {
      const raw = buf.slice(0, idx);
      buf = buf.slice(idx + 2);
      let event = 'message';
      const dataLines: string[] = [];
      for (const line of raw.split('\n')) {
        if (line.startsWith('event:')) event = line.slice(6).trim();
        else if (line.startsWith('data:')) dataLines.push(line.slice(5).trimStart());
      }
      if (dataLines.length) onEvent({ event, data: dataLines.join('\n') });
    }
  }
}
