// Renders a clip to a video file in the webview (SPEC-n32 section 3.1). drawFrame paints each frame on a canvas,
// WebCodecs H.264 encodes the frames and muxMp4 writes the MP4. When H.264 is not there, or the encoder hands the
// chunks back out of order, the same frames are recorded in real time with MediaRecorder. Nothing here sends data
// anywhere: the output is one Blob the person saves or throws away.
import { CLIP_HEIGHT, CLIP_WIDTH, drawFrame, type Ctx2D } from './clipDraw';
import { GuardCounter } from './clipGuard';
import { CLIP_MAX_MS, type Timeline } from './clipTimeline';
import type { ClipPlan } from './clipTypes';
import { muxMp4, type Sample } from './mp4Mux';

/** Frames per second of the clip. */
export const CLIP_FPS = 30;
/** WebCodecs timestamps are microseconds. Frame i gets i * this. */
const FRAME_TIMESTAMP_STEP_US = 33333;
/** The MP4 track runs at 30000 ticks per second, so one frame lasts 1000 ticks. */
const TIMESCALE = 30_000;
const TICKS_PER_FRAME = 1_000;
const BITRATE = 1_800_000;
const KEY_FRAME_EVERY = 60;
/** Back-pressure: wait while the encoder holds more frames than this. */
const MAX_QUEUE = 8;
/** Every this many frames the render gives the page a moment and reports progress. */
const YIELD_EVERY = 10;
/** H.264 profiles tried in this order; the first one the encoder accepts wins. */
const H264_CODECS = ['avc1.640028', 'avc1.4d0028', 'avc1.42001f'];
const MIME_MP4 = 'video/mp4;codecs=avc1';
const MIME_WEBM = 'video/webm;codecs=vp9';
const FONT_SPECS = ['16px "JetBrains Mono"', '16px Inter', '16px Doto'];
const NO_VIDEO = 'Your system cannot make video here.';

/* ---------- the browser parts this file uses, as small shapes so tests can pass fakes ---------- */

/** The part of an encoded video chunk the render reads. */
export interface EncodedChunkLike {
  type: 'key' | 'delta';
  timestamp: number;
  byteLength: number;
  copyTo(dest: Uint8Array): void;
}

export interface EncoderConfigLike {
  codec: string;
  width: number;
  height: number;
  bitrate: number;
  framerate: number;
  latencyMode: 'realtime' | 'quality';
  avc: { format: 'avc' | 'annexb' };
}

export interface FrameLike {
  close(): void;
}

export interface EncoderLike {
  readonly encodeQueueSize: number;
  configure(config: EncoderConfigLike): void;
  encode(frame: FrameLike, options: { keyFrame: boolean }): void;
  flush(): Promise<void>;
  close(): void;
}

export interface EncoderInit {
  output(chunk: EncodedChunkLike, meta?: { decoderConfig?: { description?: ArrayBuffer | ArrayBufferView } }): void;
  error(error: Error): void;
}

export interface EncoderCtor {
  new (init: EncoderInit): EncoderLike;
  isConfigSupported(config: EncoderConfigLike): Promise<{ supported?: boolean }>;
}

/** A canvas the frames are drawn on. */
export interface FrameCanvas {
  width: number;
  height: number;
  getContext(kind: '2d'): Ctx2D | null;
}

/** A canvas in the page. MediaRecorder records one of these through captureStream. */
export interface ElementCanvas extends FrameCanvas {
  captureStream(fps: number): unknown;
}

export interface VideoFrameCtor {
  new (source: FrameCanvas, init: { timestamp: number }): FrameLike;
}

export interface OffscreenCanvasCtor {
  new (width: number, height: number): FrameCanvas;
}

export interface RecorderLike {
  ondataavailable: ((event: { data: Blob }) => void) | null;
  onstop: (() => void) | null;
  onerror: ((event: unknown) => void) | null;
  start(): void;
  stop(): void;
}

export interface RecorderCtor {
  new (stream: unknown, options: { mimeType: string; videoBitsPerSecond: number }): RecorderLike;
  isTypeSupported?(mime: string): boolean;
}

/**
 * Everything the render takes from the browser. Each one defaults to the global of the same name (or the page), so
 * the app passes nothing and the tests pass fakes.
 */
export interface EncodeEnv {
  VideoEncoder?: EncoderCtor;
  VideoFrame?: VideoFrameCtor;
  OffscreenCanvas?: OffscreenCanvasCtor;
  MediaRecorder?: RecorderCtor;
  /** Makes a canvas in the page (the MediaRecorder path records one). Default: document.createElement. */
  makeCanvas?: () => ElementCanvas;
  /** Waits until the fonts are ready. Default: document.fonts. It never rejects. */
  loadFonts?: () => Promise<void>;
  /** Waits in real time on the MediaRecorder path. Default: setTimeout. */
  sleep?: (ms: number) => Promise<void>;
}

export interface RenderClipArgs {
  plan: ClipPlan;
  timeline: Timeline;
  /** Stops the render. The render rejects with an AbortError and the encoder is closed. */
  signal: AbortSignal;
  /** Called with a fraction from 0 to 1. */
  onProgress(p: number): void;
  env?: EncodeEnv;
}

export interface RenderClipResult {
  blob: Blob;
  ext: 'mp4' | 'webm';
  /** How many distinct drawn strings the second wall hid. */
  guardHits: number;
  /** True when the clip was recorded in real time (no H.264 encoder was used). */
  usedFallback: boolean;
}

/** Counts the distinct strings the second wall changed. A caption is drawn on many frames, so a plain count would overstate. */
class DistinctGuard extends GuardCounter {
  private readonly hidden = new Set<string>();

  override guard(text: string): string {
    const out = super.guard(text);
    if (out !== text) this.hidden.add(text);
    return out;
  }

  get distinct(): number {
    return this.hidden.size;
  }
}

/** Frame count for a timeline length: 28 000 ms is 840 frames at 30 fps. */
export function frameCount(totalMs: number): number {
  return Math.max(1, Math.round((totalMs * CLIP_FPS) / 1000));
}

/** Draws the clip, encodes it and returns the file. Throws for a clip over 30 s, and for a cancelled or impossible render. */
export async function renderClip(args: RenderClipArgs): Promise<RenderClipResult> {
  const { plan, timeline, signal, onProgress } = args;
  if (timeline.totalMs > CLIP_MAX_MS) throw new Error('This clip is longer than 30 seconds.');
  throwIfAborted(signal);
  const env = resolveEnv(args.env);
  await env.loadFonts();
  throwIfAborted(signal);

  const job: Job = { plan, timeline, signal, onProgress, frames: frameCount(timeline.totalMs) };
  const h264 = await tryH264(env, job);
  if (h264) return h264;
  return recordFallback(env, job);
}

interface Job {
  plan: ClipPlan;
  timeline: Timeline;
  signal: AbortSignal;
  onProgress(p: number): void;
  frames: number;
}

interface ResolvedEnv {
  Encoder?: EncoderCtor;
  Frame?: VideoFrameCtor;
  OffscreenCanvas?: OffscreenCanvasCtor;
  Recorder?: RecorderCtor;
  makeCanvas: () => ElementCanvas;
  loadFonts: () => Promise<void>;
  sleep: (ms: number) => Promise<void>;
}

/** The browser globals, read when the render starts (not when the module loads). */
interface GlobalsLike {
  VideoEncoder?: EncoderCtor;
  VideoFrame?: VideoFrameCtor;
  OffscreenCanvas?: OffscreenCanvasCtor;
  MediaRecorder?: RecorderCtor;
}

function resolveEnv(env: EncodeEnv = {}): ResolvedEnv {
  const g = globalThis as unknown as GlobalsLike;
  return {
    Encoder: env.VideoEncoder ?? g.VideoEncoder,
    Frame: env.VideoFrame ?? g.VideoFrame,
    OffscreenCanvas: env.OffscreenCanvas ?? g.OffscreenCanvas,
    Recorder: env.MediaRecorder ?? g.MediaRecorder,
    makeCanvas: env.makeCanvas ?? defaultMakeCanvas,
    loadFonts: env.loadFonts ?? loadFontsDefault,
    sleep: env.sleep ?? defaultSleep,
  };
}

function defaultMakeCanvas(): ElementCanvas {
  return document.createElement('canvas') as unknown as ElementCanvas;
}

/** A failed font load keeps the system font; the preview still plays. */
async function loadFontsDefault(): Promise<void> {
  try {
    const fonts = globalThis.document?.fonts;
    if (!fonts) return;
    await Promise.all(FONT_SPECS.map((spec) => fonts.load(spec).catch(() => [])));
  } catch {
    /* no font API: keep the system fonts */
  }
}

function defaultSleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function tick(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function throwIfAborted(signal: AbortSignal): void {
  if (!signal.aborted) return;
  const error = new Error('The clip was cancelled.');
  error.name = 'AbortError';
  throw error;
}

function frameTimeMs(i: number): number {
  return (i * 1000) / CLIP_FPS;
}

function encoderConfig(codec: string): EncoderConfigLike {
  return {
    codec,
    width: CLIP_WIDTH,
    height: CLIP_HEIGHT,
    bitrate: BITRATE,
    framerate: CLIP_FPS,
    // Realtime keeps the encoder from reordering frames (B-frames), so chunks come back in order.
    latencyMode: 'realtime',
    avc: { format: 'avc' },
  };
}

/** The first H.264 profile the encoder accepts, or null when none does (or there is no encoder). */
async function pickCodec(env: ResolvedEnv): Promise<string | null> {
  const Encoder = env.Encoder;
  if (!Encoder || !env.Frame) return null;
  for (const codec of H264_CODECS) {
    try {
      const result = await Encoder.isConfigSupported(encoderConfig(codec));
      if (result?.supported) return codec;
    } catch {
      /* this profile is not there: try the next one */
    }
  }
  return null;
}

/** WebCodecs H.264 path. Returns null when it cannot finish, so the caller records the fallback instead. */
async function tryH264(env: ResolvedEnv, job: Job): Promise<RenderClipResult | null> {
  const codec = await pickCodec(env);
  if (!codec || !env.Encoder || !env.Frame) return null;
  try {
    const canvas = frameCanvas(env);
    const out = await encodeH264(env.Encoder, env.Frame, canvas, job, codec);
    return { blob: out.blob, ext: 'mp4', guardHits: out.guard.distinct, usedFallback: false };
  } catch {
    // A cancel is not a failure: it goes up. Anything else (no output, bad order, encoder error) takes the fallback.
    throwIfAborted(job.signal);
    return null;
  }
}

/** The canvas the H.264 path draws on. An OffscreenCanvas when there is one, else a page canvas. */
function frameCanvas(env: ResolvedEnv): FrameCanvas {
  if (env.OffscreenCanvas) return new env.OffscreenCanvas(CLIP_WIDTH, CLIP_HEIGHT);
  const canvas = env.makeCanvas();
  canvas.width = CLIP_WIDTH;
  canvas.height = CLIP_HEIGHT;
  return canvas;
}

interface EncodeState {
  avcC: Uint8Array | null;
  lastTs: number;
  /** Why the encoder gave up, or null while it is fine. */
  broken: string | null;
  samples: Sample[];
}

async function encodeH264(
  Encoder: EncoderCtor,
  Frame: VideoFrameCtor,
  canvas: FrameCanvas,
  job: Job,
  codec: string,
): Promise<{ blob: Blob; guard: DistinctGuard }> {
  const { plan, timeline, signal, onProgress, frames } = job;
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('No 2D canvas.');
  const guard = new DistinctGuard();
  const state: EncodeState = { avcC: null, lastTs: -1, broken: null, samples: [] };

  const encoder = new Encoder({
    output(chunk, meta) {
      const description = meta?.decoderConfig?.description;
      if (description && !state.avcC) state.avcC = toBytes(description);
      if (chunk.timestamp <= state.lastTs) state.broken = 'The encoder returned frames out of order.';
      state.lastTs = chunk.timestamp;
      const data = new Uint8Array(chunk.byteLength);
      chunk.copyTo(data);
      state.samples.push({ data, durationTicks: TICKS_PER_FRAME, key: chunk.type === 'key' });
    },
    error(error) {
      state.broken = error.message || 'The encoder failed.';
    },
  });

  try {
    encoder.configure(encoderConfig(codec));
    for (let i = 0; i < frames; i++) {
      throwIfAborted(signal);
      if (state.broken) throw new Error(state.broken);
      drawFrame(ctx, plan, timeline, frameTimeMs(i), { guard });
      const frame = new Frame(canvas, { timestamp: i * FRAME_TIMESTAMP_STEP_US });
      encoder.encode(frame, { keyFrame: i % KEY_FRAME_EVERY === 0 });
      frame.close();
      while (encoder.encodeQueueSize > MAX_QUEUE) {
        throwIfAborted(signal);
        await tick();
      }
      if (i % YIELD_EVERY === 0) {
        onProgress(i / frames);
        await tick();
      }
    }
    throwIfAborted(signal);
    await encoder.flush();
    if (state.broken) throw new Error(state.broken);
    const avcC = state.avcC;
    if (!avcC || state.samples.length !== frames) throw new Error('The encoder did not return every frame.');
    const bytes = muxMp4({ width: CLIP_WIDTH, height: CLIP_HEIGHT, timescale: TIMESCALE, avcC, samples: state.samples });
    onProgress(1);
    // muxMp4 builds a fresh buffer (never a shared one), so the cast is safe.
    return { blob: new Blob([bytes as Uint8Array<ArrayBuffer>], { type: 'video/mp4' }), guard };
  } finally {
    closeQuietly(encoder);
  }
}

function toBytes(description: ArrayBuffer | ArrayBufferView): Uint8Array {
  return ArrayBuffer.isView(description)
    ? new Uint8Array(description.buffer, description.byteOffset, description.byteLength)
    : new Uint8Array(description);
}

function closeQuietly(encoder: EncoderLike): void {
  try {
    encoder.close();
  } catch {
    /* already closed */
  }
}

/** MediaRecorder path: the same frames, drawn and recorded in real time. */
async function recordFallback(env: ResolvedEnv, job: Job): Promise<RenderClipResult> {
  const { plan, timeline, signal, onProgress, frames } = job;
  const Recorder = env.Recorder;
  if (!Recorder) throw new Error(NO_VIDEO);
  const mime = pickMime(Recorder);
  const canvas = env.makeCanvas();
  canvas.width = CLIP_WIDTH;
  canvas.height = CLIP_HEIGHT;
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error(NO_VIDEO);

  let recorder: RecorderLike;
  try {
    recorder = new Recorder(canvas.captureStream(CLIP_FPS), { mimeType: mime, videoBitsPerSecond: BITRATE });
  } catch {
    throw new Error(NO_VIDEO);
  }
  const guard = new DistinctGuard();
  const parts: Blob[] = [];
  recorder.ondataavailable = (event) => {
    if (event.data.size > 0) parts.push(event.data);
  };
  const stopped = new Promise<void>((resolve, reject) => {
    recorder.onstop = () => resolve();
    recorder.onerror = () => reject(new Error('The recording failed.'));
  });
  stopped.catch(() => undefined);
  recorder.start();

  const frameMs = 1000 / CLIP_FPS;
  try {
    for (let i = 0; i < frames; i++) {
      throwIfAborted(signal);
      drawFrame(ctx, plan, timeline, frameTimeMs(i), { guard });
      if (i % YIELD_EVERY === 0) onProgress(i / frames);
      await env.sleep(frameMs);
    }
  } catch (err) {
    stopRecorder(recorder);
    await stopped.catch(() => undefined);
    throw err;
  }
  recorder.stop();
  await stopped;
  onProgress(1);

  const ext = mime === MIME_MP4 ? 'mp4' : 'webm';
  const blob = new Blob(parts, { type: ext === 'mp4' ? 'video/mp4' : 'video/webm' });
  return { blob, ext, guardHits: guard.distinct, usedFallback: true };
}

/** MP4 when the browser can record it, else WebM. */
function pickMime(Recorder: RecorderCtor): string {
  const supported = (mime: string) => typeof Recorder.isTypeSupported === 'function' && Recorder.isTypeSupported(mime);
  return supported(MIME_MP4) ? MIME_MP4 : MIME_WEBM;
}

function stopRecorder(recorder: RecorderLike): void {
  try {
    recorder.stop();
  } catch {
    /* already stopped */
  }
}
