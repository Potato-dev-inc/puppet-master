// The three viewer calls. Every call is a GET with the read token and the seat id in headers. Answers are
// checked by shape; relay text is never shown, only a short code the page turns into a fixed sentence.

import type { FrameKind, RoomBatch, RoomFrame } from './types';
import type { RoomLink } from './link';

export type RoomErrorCode = 'missing' | 'ended' | 'full' | 'network' | 'bad';

/** A failed call. `code` is all the page needs to choose its sentence. */
export class RoomError extends Error {
  readonly code: RoomErrorCode;

  constructor(code: RoomErrorCode) {
    super(code);
    this.name = 'RoomError';
    this.code = code;
  }
}

export interface Api {
  /** The current snapshot, or null when the owner has not sent one yet. */
  snapshot(): Promise<RoomFrame | null>;
  /** Frames after `after`, waiting up to `waitS` seconds for new ones. */
  frames(after: number, waitS: number): Promise<RoomBatch>;
}

export type FetchLike = (input: string, init?: RequestInit) => Promise<Response>;

export interface ApiOptions {
  origin: string;
  link: RoomLink;
  seat: string;
  fetch: FetchLike;
  /** Longest wait for one call. Above the 25 s long-poll limit of the relay. */
  timeoutMs?: number;
}

const FRAME_KINDS: readonly FrameKind[] = ['snap', 'delta', 'state'];

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function toFrame(value: unknown): RoomFrame | null {
  if (!isObject(value)) {
    return null;
  }
  const { v, room, seq, kind, nonce, ct } = value;
  const ok =
    typeof v === 'number' &&
    typeof room === 'string' &&
    typeof seq === 'number' &&
    Number.isSafeInteger(seq) &&
    seq >= 1 &&
    typeof kind === 'string' &&
    FRAME_KINDS.includes(kind as FrameKind) &&
    typeof nonce === 'string' &&
    typeof ct === 'string';
  if (!ok) {
    return null;
  }
  return { v, room, seq, kind: kind as FrameKind, nonce, ct } as RoomFrame;
}

function toBatch(value: unknown): RoomBatch {
  if (!isObject(value) || !Array.isArray(value.frames)) {
    throw new RoomError('bad');
  }
  const frames: RoomFrame[] = [];
  for (const raw of value.frames) {
    const frame = toFrame(raw);
    if (frame) {
      frames.push(frame);
    }
  }
  const next = typeof value.next === 'number' && Number.isSafeInteger(value.next) ? value.next : 0;
  const ended = typeof value.ended === 'string' ? value.ended : null;
  return { frames, next, ended };
}

async function readJson(res: Response): Promise<unknown> {
  try {
    return JSON.parse(await res.text());
  } catch {
    throw new RoomError('bad');
  }
}

/** Maps a non-200 status to the error code the page shows. */
function failure(status: number, rateLimitIsFull: boolean): RoomError {
  if (status === 404) {
    return new RoomError('missing');
  }
  if (status === 410) {
    return new RoomError('ended');
  }
  if (status === 429 && rateLimitIsFull) {
    return new RoomError('full');
  }
  if (status >= 500 || status === 429) {
    return new RoomError('network');
  }
  return new RoomError('bad');
}

export function makeApi(options: ApiOptions): Api {
  const { origin, link, seat, fetch, timeoutMs = 35_000 } = options;

  async function get(path: string): Promise<Response> {
    try {
      return await fetch(origin + path, {
        method: 'GET',
        headers: { 'x-pm-room-read': link.readToken, 'x-pm-room-seat': seat },
        cache: 'no-store',
        credentials: 'omit',
        redirect: 'error',
        referrerPolicy: 'no-referrer',
        signal: AbortSignal.timeout(timeoutMs),
      });
    } catch {
      throw new RoomError('network');
    }
  }

  return {
    async snapshot() {
      const res = await get(`/v1/rooms/${link.room}/snapshot`);
      if (res.status === 204) {
        return null;
      }
      if (!res.ok) {
        throw failure(res.status, true);
      }
      const frame = toFrame(await readJson(res));
      if (!frame) {
        throw new RoomError('bad');
      }
      return frame;
    },
    async frames(after, waitS) {
      const res = await get(`/v1/rooms/${link.room}/frames?after=${after}&wait=${waitS}`);
      if (!res.ok) {
        throw failure(res.status, false);
      }
      return toBatch(await readJson(res));
    },
  };
}
