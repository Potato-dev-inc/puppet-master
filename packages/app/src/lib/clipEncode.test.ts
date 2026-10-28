import { describe, expect, it, beforeEach } from 'vitest';
import type { Ctx2D } from './clipDraw';
import {
  frameCount,
  renderClip,
  type EncodeEnv,
  type EncodedChunkLike,
  type EncoderCtor,
  type EncoderConfigLike,
  type EncoderInit,
  type RecorderCtor,
  type RenderClipArgs,
} from './clipEncode';
import { buildTimeline, type Timeline } from './clipTimeline';
import type { ClipPlan } from './clipTypes';

// A fake secret is built at run time, never written out as literal text.
const SECRET = `sk-ant-api03-${'A1'.repeat(10)}`;

/** A 2D context that does nothing. drawFrame only needs the methods to exist. */
function fakeCtx(): Ctx2D {
  const ctx = {
    fillStyle: '#000000',
    font: '10px sans-serif',
    textAlign: 'left',
    textBaseline: 'alphabetic',
    globalAlpha: 1,
    fillRect() {},
    fillText() {},
    measureText(text: string) {
      return { width: text.length * 6 };
    },
    beginPath() {},
    moveTo() {},
    arcTo() {},
    closePath() {},
    fill() {},
    rect() {},
    clip() {},
    save() {},
    restore() {},
  };
  return ctx as unknown as Ctx2D;
}

/** A canvas for the frames and for the MediaRecorder path. */
class FakeCanvas {
  width = 0;
  height = 0;
  getContext(): Ctx2D {
    return fakeCtx();
  }
  captureStream(): unknown {
    return { fake: true };
  }
}

/** A plan with a terminal-free replay: title 3 s + activity 17.5 s + result 5 s + end 2.5 s = 28 s. */
function plan(request = 'Fix the login page'): ClipPlan {
  return {
    token: 'tok',
    agent: 'Test Agent',
    request,
    turnMs: 60_000,
    outcome: 'completed',
    replay: { kind: 'activity', lengthMs: 17_500 },
    steps: [],
    result: { files: null, checks: null, sentence: 'Done.' },
    notes: [],
    hidden: 0,
  };
}

/** A fake H.264 encoder. It records every frame it is given and emits one chunk per frame. */
class FakeEncoder {
  static supported = new Set<string>(['avc1.640028']);
  static instances: FakeEncoder[] = [];
  /** When set, the chunk for this frame index gets a timestamp before the one before it. */
  static reorderAt: number | null = null;
  /** When set, the encoder reports an error on this frame index. */
  static failAt: number | null = null;

  encodeQueueSize = 0;
  encoded: { ts: number; key: boolean }[] = [];
  configs: EncoderConfigLike[] = [];
  closed = false;
  private readonly init: EncoderInit;
  private emitted = 0;

  constructor(init: EncoderInit) {
    this.init = init;
    FakeEncoder.instances.push(this);
  }

  static isConfigSupported(config: EncoderConfigLike): Promise<{ supported: boolean }> {
    return Promise.resolve({ supported: FakeEncoder.supported.has(config.codec) });
  }

  configure(config: EncoderConfigLike): void {
    this.configs.push(config);
  }

  encode(frame: { timestamp?: number; close(): void }, options: { keyFrame: boolean }): void {
    const index = this.encoded.length;
    this.encoded.push({ ts: frame.timestamp ?? -1, key: options.keyFrame });
    if (FakeEncoder.failAt === index) {
      this.init.error(new Error('fake encoder failed'));
      return;
    }
    const reorder = FakeEncoder.reorderAt === index;
    const ts = reorder ? 0 : (frame.timestamp ?? 0);
    const first = this.emitted === 0;
    this.emitted += 1;
    const chunk: EncodedChunkLike = {
      type: options.keyFrame ? 'key' : 'delta',
      timestamp: ts,
      byteLength: 6,
      copyTo(dest: Uint8Array) {
        dest.set([0, 0, 0, 2, 0x65, 0x88]);
      },
    };
    // The first chunk carries the H.264 config record (an avcC payload).
    const meta = first ? { decoderConfig: { description: new Uint8Array([1, 0x64, 0, 0x28, 0xff, 0xe1, 0, 0]) } } : undefined;
    this.init.output(chunk, meta);
  }

  flush(): Promise<void> {
    return Promise.resolve();
  }

  close(): void {
    if (this.closed) throw new Error('already closed');
    this.closed = true;
  }
}

class FakeVideoFrame {
  readonly timestamp: number;
  closed = false;
  constructor(_source: unknown, init: { timestamp: number }) {
    this.timestamp = init.timestamp;
  }
  close(): void {
    this.closed = true;
  }
}

/** A fake MediaRecorder. `stop()` hands over one small blob and fires onstop, like a real one. */
class FakeRecorder {
  static types = new Set<string>(['video/mp4;codecs=avc1', 'video/webm;codecs=vp9']);
  static used: string[] = [];
  static isTypeSupported(mime: string): boolean {
    return FakeRecorder.types.has(mime);
  }

  ondataavailable: ((event: { data: Blob }) => void) | null = null;
  onstop: (() => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  started = false;
  stopped = false;

  constructor(_stream: unknown, options: { mimeType: string; videoBitsPerSecond: number }) {
    FakeRecorder.used.push(options.mimeType);
  }
  start(): void {
    this.started = true;
  }
  stop(): void {
    this.stopped = true;
    this.ondataavailable?.({ data: new Blob([new Uint8Array([1, 2, 3])]) });
    this.onstop?.();
  }
}

/** The env for a render. Every browser part is a fake, and waits are instant. */
function fakeEnv(over: Partial<EncodeEnv> = {}): EncodeEnv {
  return {
    VideoEncoder: FakeEncoder as unknown as EncoderCtor,
    VideoFrame: FakeVideoFrame as unknown as EncodeEnv['VideoFrame'],
    OffscreenCanvas: FakeCanvas as unknown as EncodeEnv['OffscreenCanvas'],
    MediaRecorder: FakeRecorder as unknown as RecorderCtor,
    makeCanvas: () => new FakeCanvas() as unknown as ReturnType<NonNullable<EncodeEnv['makeCanvas']>>,
    loadFonts: async () => undefined,
    sleep: async () => undefined,
    ...over,
  };
}

function timelineOf(p: ClipPlan): Timeline {
  return buildTimeline(p);
}

function args(over: Partial<RenderClipArgs> = {}): RenderClipArgs {
  const p = over.plan ?? plan();
  return {
    plan: p,
    timeline: timelineOf(p),
    signal: new AbortController().signal,
    onProgress: () => undefined,
    env: fakeEnv(),
    ...over,
  };
}

/** Reads a Blob's bytes with FileReader (jsdom's Blob has no arrayBuffer()). */
function bytesOf(blob: Blob): Promise<Uint8Array> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(new Uint8Array(reader.result as ArrayBuffer));
    reader.onerror = () => reject(reader.error);
    reader.readAsArrayBuffer(blob);
  });
}

const ascii = (bytes: Uint8Array, at: number, len: number) => String.fromCharCode(...bytes.subarray(at, at + len));

// A full 28 s render draws 840 frames and gives the event loop a turn every 10 frames (84 setTimeout(0)
// yields). On Windows each yield can wait a whole timer tick (about 15-20 ms), so one render takes over a
// second even on an idle PC and several seconds on a busy one. The default 5 s limit is too tight for that.
// Only the tests that render the whole clip get this longer limit; the short renders keep the default.
const FULL_RENDER_MS = 30_000;

beforeEach(() => {
  FakeEncoder.supported = new Set(['avc1.640028']);
  FakeEncoder.instances = [];
  FakeEncoder.reorderAt = null;
  FakeEncoder.failAt = null;
  FakeRecorder.types = new Set(['video/mp4;codecs=avc1', 'video/webm;codecs=vp9']);
  FakeRecorder.used = [];
});

describe('frameCount', () => {
  it('is 30 frames per second', () => {
    expect(frameCount(28_000)).toBe(840);
    expect(frameCount(1_000)).toBe(30);
    expect(frameCount(0)).toBe(1);
  });
});

describe('renderClip on the H.264 path', () => {
  it('encodes a 28 s clip as 840 frames at increasing timestamps, with a key frame every 60', async () => {
    const a = args();
    expect(a.timeline.totalMs).toBe(28_000);
    const result = await renderClip(a);

    expect(result.usedFallback).toBe(false);
    expect(result.ext).toBe('mp4');
    expect(result.blob.type).toBe('video/mp4');
    expect(FakeRecorder.used).toEqual([]);

    const enc = FakeEncoder.instances[0]!;
    expect(enc.encoded).toHaveLength(840);
    expect(enc.encoded[0]!.ts).toBe(0);
    for (let i = 1; i < enc.encoded.length; i++) {
      expect(enc.encoded[i]!.ts).toBeGreaterThan(enc.encoded[i - 1]!.ts);
      expect(enc.encoded[i]!.ts).toBe(i * 33333);
    }
    expect(enc.encoded.filter((f) => f.key).length).toBe(14);
    expect(enc.encoded[60]!.key).toBe(true);
    expect(enc.encoded[1]!.key).toBe(false);
    expect(enc.configs[0]).toMatchObject({ codec: 'avc1.640028', width: 1280, height: 720, framerate: 30 });
    expect(enc.closed).toBe(true);
  }, FULL_RENDER_MS);

  it('writes an MP4 whose first box is ftyp and whose second box is moov', async () => {
    const result = await renderClip(args());
    const bytes = await bytesOf(result.blob);
    expect(ascii(bytes, 4, 4)).toBe('ftyp');
    const ftypSize = ((bytes[0]! << 24) | (bytes[1]! << 16) | (bytes[2]! << 8) | bytes[3]!) >>> 0;
    expect(ascii(bytes, ftypSize + 4, 4)).toBe('moov');
    // 840 samples of 6 bytes each plus the boxes: the file is well above a kilobyte.
    expect(bytes.length).toBeGreaterThan(1000);
  }, FULL_RENDER_MS);

  it('reports progress from 0 to 1 and does not keep drawing after an abort', async () => {
    const controller = new AbortController();
    const seen: number[] = [];
    const a = args({
      signal: controller.signal,
      onProgress: (p) => {
        seen.push(p);
        if (p >= 0.3) controller.abort();
      },
    });
    await expect(renderClip(a)).rejects.toMatchObject({ name: 'AbortError' });

    const enc = FakeEncoder.instances[0]!;
    expect(enc.encoded.length).toBeLessThan(840);
    expect(enc.closed).toBe(true);
    expect(seen[0]).toBe(0);
    expect(FakeRecorder.used).toEqual([]);
  });

  it('closes the encoder when an abort comes before the first frame', async () => {
    const controller = new AbortController();
    controller.abort();
    await expect(renderClip(args({ signal: controller.signal }))).rejects.toMatchObject({ name: 'AbortError' });
    expect(FakeEncoder.instances).toHaveLength(0);
  });

  it('counts distinct hidden strings, not every frame that drew them', async () => {
    const clean = await renderClip(args());
    expect(clean.guardHits).toBe(0);

    const dirty = await renderClip(args({ plan: plan(`Use ${SECRET} please`) }));
    expect(dirty.guardHits).toBeGreaterThan(0);
    expect(dirty.guardHits).toBeLessThan(10);
  }, FULL_RENDER_MS);
});

describe('renderClip fallback', () => {
  it('uses MediaRecorder when no H.264 profile is supported, and says so', async () => {
    FakeEncoder.supported = new Set();
    const result = await renderClip(args());
    expect(result.usedFallback).toBe(true);
    expect(result.ext).toBe('mp4');
    expect(result.blob.type).toBe('video/mp4');
    expect(FakeRecorder.used).toEqual(['video/mp4;codecs=avc1']);
    expect(FakeEncoder.instances).toHaveLength(0);
  });

  it('records WebM when the browser cannot record MP4', async () => {
    FakeEncoder.supported = new Set();
    FakeRecorder.types = new Set(['video/webm;codecs=vp9']);
    const result = await renderClip(args());
    expect(result.usedFallback).toBe(true);
    expect(result.ext).toBe('webm');
    expect(result.blob.type).toBe('video/webm');
    expect(FakeRecorder.used).toEqual(['video/webm;codecs=vp9']);
  });

  it('falls back when the encoder hands back a frame out of order', async () => {
    FakeEncoder.reorderAt = 5;
    const result = await renderClip(args());
    expect(result.usedFallback).toBe(true);
    expect(FakeRecorder.used).toHaveLength(1);
    expect(FakeEncoder.instances[0]!.closed).toBe(true);
  });

  it('falls back when the encoder reports an error', async () => {
    FakeEncoder.failAt = 3;
    const result = await renderClip(args());
    expect(result.usedFallback).toBe(true);
    expect(FakeEncoder.instances[0]!.closed).toBe(true);
  });

  it('throws a plain sentence when there is no encoder and no recorder', async () => {
    FakeEncoder.supported = new Set();
    await expect(renderClip(args({ env: fakeEnv({ VideoEncoder: undefined, MediaRecorder: undefined }) }))).rejects.toThrow(
      'Your system cannot make video here.',
    );
  });

  it('stops the recorder when the person cancels a fallback render', async () => {
    FakeEncoder.supported = new Set();
    const controller = new AbortController();
    const a = args({
      signal: controller.signal,
      onProgress: (p) => {
        if (p >= 0.2) controller.abort();
      },
    });
    await expect(renderClip(a)).rejects.toMatchObject({ name: 'AbortError' });
    expect(FakeRecorder.used).toHaveLength(1);
  });
});

describe('renderClip limits', () => {
  it('refuses a clip longer than 30 seconds', async () => {
    const p = plan();
    const tl = { ...timelineOf(p), totalMs: 30_001 };
    await expect(renderClip(args({ plan: p, timeline: tl }))).rejects.toThrow('longer than 30 seconds');
    expect(FakeEncoder.instances).toHaveLength(0);
  });

  it('waits for the fonts before the first frame', async () => {
    let fonts = 0;
    await renderClip(
      args({
        env: fakeEnv({
          loadFonts: async () => {
            fonts += 1;
          },
        }),
      }),
    );
    expect(fonts).toBe(1);
  }, FULL_RENDER_MS);
});
