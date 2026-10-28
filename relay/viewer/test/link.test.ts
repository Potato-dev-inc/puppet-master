import { describe, expect, it } from 'vitest';
import { parseLink } from '../src/link';

const ROOM = 'a'.repeat(26);
const TOKEN = 'b'.repeat(22);
const KEY = 'c'.repeat(43);

describe('parseLink', () => {
  it('reads room, read token and content key from the fragment', () => {
    expect(parseLink(`#${ROOM}.${TOKEN}.${KEY}`)).toEqual({ room: ROOM, readToken: TOKEN, contentKey: KEY });
  });

  it('accepts the base64url symbols - and _', () => {
    const token = '-_'.repeat(11);
    const key = '-_'.repeat(21) + '-';
    expect(parseLink(`#${ROOM}.${token}.${key}`)?.readToken).toBe(token);
  });

  it.each([
    ['empty', ''],
    ['no hash sign', `${ROOM}.${TOKEN}.${KEY}`],
    ['missing key', `#${ROOM}.${TOKEN}`],
    ['room too short', `#${'a'.repeat(25)}.${TOKEN}.${KEY}`],
    ['room too long', `#${'a'.repeat(27)}.${TOKEN}.${KEY}`],
    ['token too short', `#${ROOM}.${'b'.repeat(21)}.${KEY}`],
    ['key too long', `#${ROOM}.${TOKEN}.${'c'.repeat(44)}`],
    ['standard base64 symbol', `#${ROOM}.${'+'.repeat(22)}.${KEY}`],
    ['extra part', `#${ROOM}.${TOKEN}.${KEY}.x`],
    ['path in the fragment', `#/r/${ROOM}.${TOKEN}.${KEY}`],
  ])('returns null for %s', (_name, hash) => {
    expect(parseLink(hash)).toBeNull();
  });
});
