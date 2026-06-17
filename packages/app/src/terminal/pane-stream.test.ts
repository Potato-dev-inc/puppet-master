import { describe, expect, it, vi } from 'vitest';
import { MAX_BACKLOG_CHUNKS, PaneStreamManager } from './pane-stream';

const enc = new TextEncoder();
const dec = new TextDecoder();

function bytes(text: string): Uint8Array {
  return enc.encode(text);
}

function text(chunks: Uint8Array[]): string[] {
  return chunks.map((chunk) => dec.decode(chunk));
}

describe('PaneStreamManager', () => {
  it('reseeds from raw backend scrollback before going live', async () => {
    const manager = new PaneStreamManager();
    const received: Uint8Array[] = [];
    const readRawBuffer = vi.fn(async () => Array.from(bytes('REPLAY')));

    manager.subscribe('pane-1', (chunk) => received.push(chunk), readRawBuffer);

    await vi.waitFor(() => {
      expect(readRawBuffer).toHaveBeenCalledWith('pane-1', 10_000);
      expect(text(received)).toEqual(['REPLAY']);
    });

    manager.ingest('pane-1', bytes('LIVE'));
    expect(text(received)).toEqual(['REPLAY', 'LIVE']);
  });

  it('drains chunks that arrive while raw replay is loading', async () => {
    const manager = new PaneStreamManager();
    const received: Uint8Array[] = [];
    let resolveReplay: (value: number[]) => void = () => {};
    const readRawBuffer = vi.fn(
      () => new Promise<number[]>((resolve) => {
        resolveReplay = resolve;
      }),
    );

    manager.subscribe('pane-1', (chunk) => received.push(chunk), readRawBuffer);
    manager.ingest('pane-1', bytes('DURING'));
    resolveReplay(Array.from(bytes('REPLAY')));

    await vi.waitFor(() => {
      expect(text(received)).toEqual(['REPLAY', 'DURING']);
    });
  });

  it('buffers while unsubscribed and reseeds again on remount', async () => {
    const manager = new PaneStreamManager();
    const first: Uint8Array[] = [];
    const second: Uint8Array[] = [];
    const readRawBuffer = vi
      .fn()
      .mockResolvedValueOnce(Array.from(bytes('ONE')))
      .mockResolvedValueOnce(Array.from(bytes('TWODETACHED')));

    const unsub = manager.subscribe('pane-1', (chunk) => first.push(chunk), readRawBuffer);
    await vi.waitFor(() => expect(text(first)).toEqual(['ONE']));

    unsub();
    manager.ingest('pane-1', bytes('DETACHED'));
    manager.subscribe('pane-1', (chunk) => second.push(chunk), readRawBuffer);

    await vi.waitFor(() => {
      expect(text(second)).toEqual(['TWODETACHED']);
    });
  });

  it('marks backlog truncated when detached chunks exceed the cap', async () => {
    const manager = new PaneStreamManager();
    for (let i = 0; i < MAX_BACKLOG_CHUNKS + 2; i += 1) {
      manager.ingest('pane-1', bytes(`chunk-${i}`));
    }

    const readRawBuffer = vi.fn(async () => Array.from(bytes('REPLAY')));
    const received: Uint8Array[] = [];
    manager.subscribe('pane-1', (chunk) => received.push(chunk), readRawBuffer);

    await vi.waitFor(() => {
      expect(readRawBuffer).toHaveBeenCalledWith('pane-1', 10_000);
      expect(text(received)[0]).toBe('REPLAY');
    });
  });

  it('reset clears stream state for agent switch', async () => {
    const manager = new PaneStreamManager();
    manager.ingest('pane-1', bytes('before'));
    manager.reset('pane-1');

    const received: Uint8Array[] = [];
    manager.subscribe('pane-1', (chunk) => received.push(chunk), async () => []);

    await vi.waitFor(() => {
      expect(received).toHaveLength(0);
    });
  });
});
