// @vitest-environment node
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import {
  frameAad,
  fromBase64Url,
  importKey,
  openFrame,
  toBase64Url,
  unpad,
} from '../src/crypto';
import type { RoomFrame } from '../src/types';
import { contentKey, KEY_B64, ROOM, sealFrame } from './helpers';

const vector = JSON.parse(readFileSync(join(__dirname, 'vector.json'), 'utf8')) as {
  placeholder: boolean;
  keyB64: string;
  room: string;
  seq: number;
  kind: RoomFrame['kind'];
  nonce: string;
  ct: string;
  plaintext: Record<string, unknown>;
};

describe('base64url', () => {
  it('round trips bytes without padding', () => {
    const bytes = new Uint8Array([0, 1, 2, 250, 251, 252, 253, 254, 255]);
    const text = toBase64Url(bytes);
    expect(text).not.toMatch(/[+/=]/);
    expect(Array.from(fromBase64Url(text))).toEqual(Array.from(bytes));
  });

  it('rejects standard base64 characters and impossible lengths', () => {
    expect(() => fromBase64Url('ab+d')).toThrow();
    expect(() => fromBase64Url('ab/d')).toThrow();
    expect(() => fromBase64Url('abcd==')).toThrow();
    expect(() => fromBase64Url('abcde')).toThrow();
  });
});

describe('frameAad', () => {
  it('builds the exact bytes the app binds', () => {
    const aad = frameAad({ v: 1, room: 'roomA', seq: 5, kind: 'snap', nonce: 'AAAAAAAAAAAAAAAA' });
    expect(new TextDecoder().decode(aad)).toBe('pm-room-frame-v1\n1\nroomA\n5\nsnap\nAAAAAAAAAAAAAAAA');
  });
});

describe('importKey', () => {
  it('accepts a 32-byte key and refuses other sizes', async () => {
    await expect(importKey(KEY_B64)).resolves.toBeDefined();
    await expect(importKey(toBase64Url(new Uint8Array(31)))).rejects.toThrow('bad key length');
    await expect(importKey(toBase64Url(new Uint8Array(33)))).rejects.toThrow('bad key length');
  });
});

describe('unpad', () => {
  it('returns the bytes named by the length prefix', () => {
    const padded = new Uint8Array([0, 0, 0, 3, 65, 66, 67, 0, 0]);
    expect(new TextDecoder().decode(unpad(padded))).toBe('ABC');
  });

  it('refuses a length longer than the data, or too short a buffer', () => {
    expect(() => unpad(new Uint8Array([0, 0, 0, 9, 1, 2]))).toThrow('bad padding length');
    expect(() => unpad(new Uint8Array([0, 0]))).toThrow('short frame');
  });
});

describe('openFrame', () => {
  it.each(['snap', 'delta', 'state'] as const)('round trips a %s frame', async (kind) => {
    const frame = await sealFrame({ seq: 7, kind, body: { items: [{ t: 'note', id: 1, pane: 'p1', text: 'hi' }] } });
    const opened = await openFrame(await contentKey(), frame);
    expect(opened).toEqual({ v: 1, kind, items: [{ t: 'note', id: 1, pane: 'p1', text: 'hi' }] });
  });

  it('fails when the seq, room, kind, nonce or ciphertext changes', async () => {
    const key = await contentKey();
    const frame = await sealFrame({ seq: 3, kind: 'delta', body: { items: [] } });
    const changedCt = frame.ct.slice(0, 5) + (frame.ct[5] === 'A' ? 'B' : 'A') + frame.ct.slice(6);
    const tampered: RoomFrame[] = [
      { ...frame, seq: 4 },
      { ...frame, room: 'bbbbbbbbbbbbbbbbbbbbbbbbbb' },
      { ...frame, kind: 'state' },
      { ...frame, nonce: toBase64Url(new Uint8Array(12).fill(9)) },
      { ...frame, ct: changedCt },
    ];
    for (const bad of tampered) {
      await expect(openFrame(key, bad)).rejects.toThrow();
    }
  });

  it('fails with a different key', async () => {
    const frame = await sealFrame({ seq: 1, kind: 'snap', body: {} });
    const other = await importKey(toBase64Url(new Uint8Array(32).fill(8)));
    await expect(openFrame(other, frame)).rejects.toThrow();
  });

  it('refuses a nonce that is not 12 bytes', async () => {
    const frame = await sealFrame({ seq: 1, kind: 'snap', body: {} });
    await expect(openFrame(await contentKey(), { ...frame, nonce: toBase64Url(new Uint8Array(8)) })).rejects.toThrow(
      'bad nonce length',
    );
  });

  it('refuses a frame whose sealed kind differs from its envelope', async () => {
    const frame = await sealFrame({ seq: 2, kind: 'delta', body: { kind: 'state' } });
    await expect(openFrame(await contentKey(), frame)).rejects.toThrow('does not match');
  });

  it('refuses a frame of another version', async () => {
    const frame = await sealFrame({ seq: 2, kind: 'delta', body: {} });
    await expect(openFrame(await contentKey(), { ...frame, v: 2 })).rejects.toThrow('unknown frame version');
  });

  it('keeps the room id in the frame it opened from', async () => {
    const frame = await sealFrame({ seq: 1, kind: 'snap', body: {} });
    expect(frame.room).toBe(ROOM);
  });
});

describe.skipIf(vector.placeholder)('shared vector (from LR-3)', () => {
  it('opens the frame sealed by the app', async () => {
    const key = await importKey(vector.keyB64);
    const frame: RoomFrame = {
      v: 1,
      room: vector.room,
      seq: vector.seq,
      kind: vector.kind,
      nonce: vector.nonce,
      ct: vector.ct,
    };
    await expect(openFrame(key, frame)).resolves.toEqual({ v: 1, ...vector.plaintext, kind: vector.kind });
  });
});
