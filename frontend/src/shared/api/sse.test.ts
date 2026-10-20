import { describe, expect, it } from 'vitest';
import { readSse, type SseEvent } from './sse';

function streamOf(...chunks: string[]): ReadableStream<Uint8Array> {
  const enc = new TextEncoder();
  return new ReadableStream({
    start(controller) {
      for (const c of chunks) controller.enqueue(enc.encode(c));
      controller.close();
    },
  });
}

describe('readSse', () => {
  it('parses events split arbitrarily across chunks', async () => {
    const events: SseEvent[] = [];
    await readSse(
      streamOf('event: delta\ndata: {"text":"He', 'l"}\n\nevent: done\ndata: {"id":1}\n\n'),
      (e) => events.push(e),
    );
    expect(events).toEqual([
      { event: 'delta', data: '{"text":"Hel"}' },
      { event: 'done', data: '{"id":1}' },
    ]);
  });

  it('defaults the event name to message and joins multi-line data', async () => {
    const events: SseEvent[] = [];
    await readSse(streamOf('data: a\ndata: b\n\n'), (e) => events.push(e));
    expect(events).toEqual([{ event: 'message', data: 'a\nb' }]);
  });

  it('ignores a trailing partial event', async () => {
    const events: SseEvent[] = [];
    await readSse(streamOf('event: delta\ndata: {"text":"x"}'), (e) => events.push(e));
    expect(events).toEqual([]);
  });
});
