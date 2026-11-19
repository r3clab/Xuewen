/// The `data` payload of each complete server-sent event in a fetch() body
/// (EventSource cannot POST). A trailing partial event (stream cut
/// mid-message) is dropped — the caller treats a missing final event as an
/// interrupted reply. Leaving the loop early cancels the stream.
export async function* sseData(body: ReadableStream<Uint8Array>): AsyncGenerator<string> {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buf = '';
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) return;
      buf += decoder.decode(value, { stream: true });
      let idx: number;
      while ((idx = buf.indexOf('\n\n')) !== -1) {
        const raw = buf.slice(0, idx);
        buf = buf.slice(idx + 2);
        const data = raw
          .split('\n')
          .filter((line) => line.startsWith('data:'))
          .map((line) => line.slice(5).trimStart());
        if (data.length) yield data.join('\n');
      }
    }
  } finally {
    await reader.cancel();
  }
}
