import { describe, expect, it } from 'vitest';
import { sseData } from './sse';

function streamOf(...chunks: string[]): ReadableStream<Uint8Array> {
  const enc = new TextEncoder();
  return new ReadableStream({
    start(controller) {
      for (const c of chunks) controller.enqueue(enc.encode(c));
      controller.close();
    },
  });
}

async function collect(body: ReadableStream<Uint8Array>): Promise<string[]> {
  const out: string[] = [];
  for await (const data of sseData(body)) out.push(data);
  return out;
}

describe('sseData', () => {
  it('yields each event split arbitrarily across chunks', async () => {
    expect(await collect(streamOf('data: {"type":"delta","text":"He', 'l"}\n\ndata: {"type":"done","id":1}\n\n'))).toEqual([
      '{"type":"delta","text":"Hel"}',
      '{"type":"done","id":1}',
    ]);
  });

  it('joins multi-line data and skips keep-alive comments', async () => {
    expect(await collect(streamOf(':\n\ndata: a\ndata: b\n\n'))).toEqual(['a\nb']);
  });

  it('ignores a trailing partial event', async () => {
    expect(await collect(streamOf('data: {"type":"delta","text":"x"}'))).toEqual([]);
  });

  it('cancels the stream when the reader stops early', async () => {
    let cancelled = false;
    const enc = new TextEncoder();
    const body = new ReadableStream<Uint8Array>({
      pull(controller) {
        controller.enqueue(enc.encode('data: x\n\n'));
      },
      cancel() {
        cancelled = true;
      },
    });
    for await (const data of sseData(body)) {
      expect(data).toBe('x');
      break;
    }
    expect(cancelled).toBe(true);
  });
});
