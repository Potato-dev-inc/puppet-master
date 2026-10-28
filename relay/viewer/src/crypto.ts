// Opens room frames with WebCrypto. The key comes from the link's #fragment and never leaves this page.
// The AAD and the padding are the exact bytes of the app's seal (pm-relay-protocol RoomFrame::aad and
// live_room/seal.rs), so a frame sealed by the app opens here and a changed field fails the tag check.

import { FRAME_VERSION, type Frame, type FrameKind, type RoomFrame } from './types';

const AAD_PREFIX = 'pm-room-frame-v1';
const KEY_BYTES = 32;
const NONCE_BYTES = 12;
const LENGTH_BYTES = 4;
const BASE64URL = /^[A-Za-z0-9_-]*$/;

/** Decodes base64url without padding. Throws on any other character. */
export function fromBase64Url(text: string): Uint8Array<ArrayBuffer> {
  if (!BASE64URL.test(text) || text.length % 4 === 1) {
    throw new Error('bad base64url');
  }
  const std = text.replace(/-/g, '+').replace(/_/g, '/');
  const bin = atob(std + '='.repeat((4 - (std.length % 4)) % 4));
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) {
    out[i] = bin.charCodeAt(i);
  }
  return out;
}

/** Encodes bytes as base64url without padding. */
export function toBase64Url(bytes: Uint8Array): string {
  let bin = '';
  for (const byte of bytes) {
    bin += String.fromCharCode(byte);
  }
  return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

/** The associated data of a frame: the same text the app binds (`RoomFrame::aad`). */
export function frameAad(frame: Pick<RoomFrame, 'v' | 'room' | 'seq' | 'kind' | 'nonce'>): Uint8Array<ArrayBuffer> {
  const text = `${AAD_PREFIX}\n${frame.v}\n${frame.room}\n${frame.seq}\n${frame.kind}\n${frame.nonce}`;
  return new TextEncoder().encode(text);
}

/** Imports the 32-byte content key from the fragment for decryption only. */
export async function importKey(keyB64: string): Promise<CryptoKey> {
  const raw = fromBase64Url(keyB64);
  if (raw.length !== KEY_BYTES) {
    throw new Error('bad key length');
  }
  return crypto.subtle.importKey('raw', raw, { name: 'AES-GCM' }, false, ['decrypt']);
}

/** Drops the padding: 4-byte big-endian length, then the bytes, then zeros. */
export function unpad(padded: Uint8Array): Uint8Array {
  if (padded.length < LENGTH_BYTES) {
    throw new Error('short frame');
  }
  const view = new DataView(padded.buffer, padded.byteOffset, padded.byteLength);
  const length = view.getUint32(0, false);
  if (length > padded.length - LENGTH_BYTES) {
    throw new Error('bad padding length');
  }
  return padded.subarray(LENGTH_BYTES, LENGTH_BYTES + length);
}

/** The JSON must be an object of this version. A `kind` inside it, when present, must match the envelope. */
function checkBody(value: unknown, kind: FrameKind): Frame {
  if (typeof value !== 'object' || value === null) {
    throw new Error('frame is not an object');
  }
  const body = value as { v?: unknown; kind?: unknown };
  if (body.v !== FRAME_VERSION) {
    throw new Error('unknown frame version');
  }
  if (body.kind !== undefined && body.kind !== kind) {
    throw new Error('frame does not match its envelope');
  }
  return { ...(value as Frame), kind };
}

/** Decrypts one frame. Throws if the tag, the AAD, the padding or the JSON does not check out. */
export async function openFrame(key: CryptoKey, frame: RoomFrame): Promise<Frame> {
  if (frame.v !== FRAME_VERSION) {
    throw new Error('unknown frame version');
  }
  const iv = fromBase64Url(frame.nonce);
  if (iv.length !== NONCE_BYTES) {
    throw new Error('bad nonce length');
  }
  const sealed = fromBase64Url(frame.ct);
  const plain = await crypto.subtle.decrypt(
    { name: 'AES-GCM', iv, additionalData: frameAad(frame), tagLength: 128 },
    key,
    sealed,
  );
  const body = unpad(new Uint8Array(plain));
  return checkBody(JSON.parse(new TextDecoder().decode(body)), frame.kind);
}
