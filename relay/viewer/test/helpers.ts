// Test helpers. sealFrame mirrors the app's seal (live_room/seal.rs): 4-byte length, the bytes, zero padding to
// a bucket, AES-256-GCM with a 4-byte salt and an 8-byte counter as the nonce, and the frame AAD. It only
// exists to build frames for the viewer tests; the real shared vector is in vector.json.

import { frameAad, importKey, toBase64Url } from '../src/crypto';
import type { FrameKind, RoomFrame } from '../src/types';

export const ROOM = 'aaaaaaaaaaaaaaaaaaaaaaaaaa';

/** A 32-byte key of 0x07, as base64url. Used by every test that seals frames. */
export const KEY_B64 = toBase64Url(new Uint8Array(32).fill(7));

export interface SealOptions {
  key?: Uint8Array<ArrayBuffer>;
  room?: string;
  seq: number;
  kind: FrameKind;
  /** The JSON the frame carries. `v` and `kind` are added when missing. */
  body: Record<string, unknown>;
  salt?: Uint8Array;
  counter?: number;
  /** Plaintext bucket in bytes. Defaults to 1 KiB. */
  bucket?: number;
}

function padded(plain: Uint8Array, bucket: number): Uint8Array<ArrayBuffer> {
  const length = 4 + plain.length;
  const size = Math.ceil(length / bucket) * bucket;
  const out = new Uint8Array(size);
  new DataView(out.buffer).setUint32(0, plain.length, false);
  out.set(plain, 4);
  return out;
}

/** Seals a frame the way the app does. Returns the envelope the relay would send. */
export async function sealFrame(options: SealOptions): Promise<RoomFrame> {
  const keyBytes: Uint8Array<ArrayBuffer> = options.key ?? new Uint8Array(32).fill(7);
  const salt: Uint8Array = options.salt ?? new Uint8Array([1, 2, 3, 4]);
  const nonce = new Uint8Array(12);
  nonce.set(salt, 0);
  new DataView(nonce.buffer).setBigUint64(4, BigInt(options.counter ?? 1), false);

  const plainJson = JSON.stringify({ v: 1, kind: options.kind, ...options.body });
  const plain = padded(new TextEncoder().encode(plainJson), options.bucket ?? 1024);

  const frameWithoutCt = {
    v: 1,
    room: options.room ?? ROOM,
    seq: options.seq,
    kind: options.kind,
    nonce: toBase64Url(nonce),
  };
  const sealKey = await crypto.subtle.importKey('raw', keyBytes, { name: 'AES-GCM' }, false, ['encrypt']);
  const ct = await crypto.subtle.encrypt(
    { name: 'AES-GCM', iv: nonce, additionalData: frameAad(frameWithoutCt), tagLength: 128 },
    sealKey,
    plain,
  );
  return { ...frameWithoutCt, ct: toBase64Url(new Uint8Array(ct)) };
}

/** The key the frames above open with. */
export async function contentKey(): Promise<CryptoKey> {
  return importKey(KEY_B64);
}
