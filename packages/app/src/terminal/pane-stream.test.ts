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

  it('discards chunks that arrive while raw replay is loading', async () => {
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
      expect(text(received)).toEqual(['REPLAY']);
    });

    manager.ingest('pane-1', bytes('AFTER'));
    expect(text(received)).toEqual(['REPLAY', 'AFTER']);
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

  it('sizes the viewer to a screen replay before drawing it, then passes resizes in order', async () => {
    const manager = new PaneStreamManager();
    const events: string[] = [];
    const readScreen = vi.fn(async () => ({ data: bytes('SCREEN'), cols: 46, rows: 20 }));

    manager.subscribe(
      'pane-1',
      (chunk) => events.push(dec.decode(chunk)),
      readScreen,
      (cols, rows) => events.push(`resize ${cols}x${rows}`),
    );

    await vi.waitFor(() => {
      expect(events).toEqual(['resize 46x20', 'SCREEN']);
    });

    manager.ingest('pane-1', bytes('old-size output'));
    manager.ingestResize('pane-1', 46, 45);
    manager.ingest('pane-1', bytes('new-size output'));
    expect(events).toEqual(['resize 46x20', 'SCREEN', 'old-size output', 'resize 46x45', 'new-size output']);
  });

  it('ignores resizes while the replay is still loading', () => {
    const manager = new PaneStreamManager();
    const onResize = vi.fn();
    manager.subscribe('pane-1', () => {}, () => new Promise<number[]>(() => {}), onResize);

    manager.ingestResize('pane-1', 40, 20);

    expect(onResize).not.toHaveBeenCalled();
  });

  it('takes the screen again when the PTY resized while a snapshot was in flight', async () => {
    const manager = new PaneStreamManager();
    const events: string[] = [];
    let call = 0;
    const readScreen = vi.fn(async () => {
      call += 1;
      if (call === 1) {
        // Opening the tab claimed the pane: the resize arrives before this old snapshot.
        manager.ingestResize('pane-1', 46, 45);
        return { data: bytes('OLD'), cols: 121, rows: 36 };
      }
      return { data: bytes('NEW'), cols: 46, rows: 45 };
    });
    manager.subscribe('pane-1', (c) => events.push(dec.decode(c)), readScreen, (c, r) => events.push(`resize ${c}x${r}`));

    await vi.waitFor(() => expect(events).toEqual(['resize 46x45', 'NEW']));
    expect(readScreen).toHaveBeenCalledTimes(2);
    manager.ingestResize('pane-1', 46, 20);
    expect(events).toEqual(['resize 46x45', 'NEW', 'resize 46x20']);
  });

  it('does not take the screen again when the snapshot already has the new size', async () => {
    const manager = new PaneStreamManager();
    const events: string[] = [];
    const readScreen = vi.fn(async () => {
      manager.ingestResize('pane-1', 46, 45);
      return { data: bytes('NEW'), cols: 46, rows: 45 };
    });
    manager.subscribe('pane-1', (c) => events.push(dec.decode(c)), readScreen, (c, r) => events.push(`resize ${c}x${r}`));

    await vi.waitFor(() => expect(events).toEqual(['resize 46x45', 'NEW']));
    expect(readScreen).toHaveBeenCalledTimes(1);
  });

  it('applies a resize that arrived during a raw replay after drawing it', async () => {
    const manager = new PaneStreamManager();
    const events: string[] = [];
    const readRaw = vi.fn(async () => {
      manager.ingestResize('pane-1', 46, 45);
      return Array.from(bytes('RAW'));
    });
    manager.subscribe('pane-1', (c) => events.push(dec.decode(c)), readRaw, (c, r) => events.push(`resize ${c}x${r}`));

    await vi.waitFor(() => expect(events).toEqual(['RAW', 'resize 46x45']));
  });
});
