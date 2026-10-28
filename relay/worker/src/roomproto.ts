// Live Room shape checks (SPEC-n32 §3.7): TypeScript mirrors of `RoomRegistration::verify` and
// `RoomFrame::check_shape` in relay/protocol/src/room.rs, with the same messages and the same order of
// checks, so the Worker refuses exactly what the axum relay refuses.
//
// Signatures: an owner call is checked by the wasm `authenticate` (relay/protocol `verify_request`, strict),
// as for teams. The registration signature has no wasm export yet, so it is checked here with WebCrypto
// Ed25519, plus the two rules of `verify_strict` that WebCrypto does not enforce: a canonical `S` (below the
// group order) and no small-order key or `R`. Without those, a signature could be malleable.

/** The limits of relay/protocol/src/room.rs and of `Limits` in relay/server (the same numbers). */
export const ROOM = {
  readHeader: "x-pm-room-read",
  seatHeader: "x-pm-room-seat",
  /** Whole JSON body of a delta or state frame. */
  maxFrameBytes: 24 * 1024,
  /** Whole JSON body of a snapshot frame. */
  maxSnapshotBytes: 96 * 1024,
  maxRegistrationBytes: 8 * 1024,
  maxLifetimeMs: 8 * 60 * 60 * 1000,
  maxViewers: 50,
  /** A viewer seat counts as present for this long after its last request. */
  seatActiveMs: 40_000,
  /** A stopped or expired room keeps a tombstone (410) this long. */
  tombstoneMs: 60 * 60 * 1000,
  /** Owner signatures are remembered this long (a signed request is valid for 5 minutes either side). */
  seenForMs: 2 * 5 * 60 * 1000 + 1_000,
  /** Deltas kept per room, oldest dropped first: at most this many, and this many bytes. */
  ringFrames: 300,
  ringBytes: 2 * 1024 * 1024,
  /** Frames in one `GET …/frames` answer at most. */
  batchFrames: 100,
  /** Owner frames: a burst of 10, refilled at 2 a second, and 7,200 a day. */
  frameBurst: 10,
  frameRefillPerSecond: 2,
  framesPerDay: 7_200,
  /** Viewer seats per client address per room. */
  seatsPerAddress: 4,
} as const;

const encoder = new TextEncoder();
// A BOM stays in the text, so it is refused by JSON.parse, as serde_json refuses it.
const decoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });

/** Strict base64url without padding (the `URL_SAFE_NO_PAD` engine of relay/protocol). */
const B64_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

export function b64(bytes: Uint8Array): string {
  let text = "";
  for (const byte of bytes) text += String.fromCharCode(byte);
  return btoa(text).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** Decodes base64url; undefined for anything the Rust decoder would refuse. Surrounding spaces are ignored. */
export function unb64(input: string): Uint8Array | undefined {
  const text = input.trim();
  if (!/^[A-Za-z0-9_-]*$/.test(text) || text.length % 4 === 1) return undefined;
  const rest = text.length % 4;
  // A canonical encoding: the unused low bits of the last character are zero.
  if (rest === 2 && (B64_ALPHABET.indexOf(text[text.length - 1]) & 0x0f) !== 0) return undefined;
  if (rest === 3 && (B64_ALPHABET.indexOf(text[text.length - 1]) & 0x03) !== 0) return undefined;
  const standard = text.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat((4 - rest) % 4);
  const binary = atob(standard);
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) out[i] = binary.charCodeAt(i);
  return out;
}

const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/** Crockford base32, no padding (the `base32` of relay/protocol). */
export function base32(bytes: Uint8Array): string {
  let out = "";
  let buffer = 0;
  let bits = 0;
  for (const byte of bytes) {
    buffer = ((buffer << 8) | byte) & 0xffff;
    bits += 8;
    while (bits >= 5) {
      bits -= 5;
      out += CROCKFORD[(buffer >> bits) & 0x1f];
    }
  }
  if (bits > 0) out += CROCKFORD[(buffer << (5 - bits)) & 0x1f];
  return out;
}

export async function sha256(...parts: Uint8Array[]): Promise<Uint8Array> {
  const total = parts.reduce((n, part) => n + part.length, 0);
  const joined = new Uint8Array(total);
  let at = 0;
  for (const part of parts) {
    joined.set(part, at);
    at += part.length;
  }
  return new Uint8Array(await crypto.subtle.digest("SHA-256", joined));
}

/** A plausible room id (shape only): 26 lowercase Crockford base32 characters. */
export function isRoomId(id: string): boolean {
  return /^[0-9a-hjkmnp-tv-z]{26}$/.test(id);
}

/** The room id of a room public key: the first 26 characters of base32(SHA-256("pm-room-v1" || key)), lowercase. */
export async function roomIdOf(roomKey: Uint8Array): Promise<string> {
  const digest = await sha256(encoder.encode("pm-room-v1"), roomKey);
  return base32(digest).slice(0, 26).toLowerCase();
}

/** The stored hash of a read token: base64url of SHA-256("pm-room-read-v1" || token). */
export async function readHashOf(token: Uint8Array): Promise<string> {
  return b64(await sha256(encoder.encode("pm-room-read-v1"), token));
}

/** A seat id from `x-pm-room-seat`: 1 to 64 letters, digits, `-` or `_`. */
export function isSeatId(seat: string): boolean {
  return /^[A-Za-z0-9_-]{1,64}$/.test(seat);
}

// ----------------------------------------------------------------------------- registration

/** `RoomRegistration`: the field names are snake_case on the wire (no serde rename in the Rust type). */
export interface Registration {
  v: number;
  room: string;
  key: string;
  expires_at_ms: number;
  max_viewers: number;
  read_hash: string;
  sig: string;
}

const isUint = (value: unknown, max: number): value is number =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0 && value <= max;

/** The registration as the Rust type reads it, or undefined (a `bad registration`). */
export function parseRegistration(body: Uint8Array): Registration | undefined {
  let raw: unknown;
  try {
    raw = JSON.parse(decoder.decode(body));
  } catch {
    return undefined;
  }
  if (typeof raw !== "object" || raw === null) return undefined;
  const r = raw as Record<string, unknown>;
  if (
    !isUint(r.v, 255) ||
    typeof r.room !== "string" ||
    typeof r.key !== "string" ||
    !isUint(r.expires_at_ms, Number.MAX_SAFE_INTEGER) ||
    !isUint(r.max_viewers, 0xffffffff) ||
    typeof r.read_hash !== "string" ||
    typeof r.sig !== "string"
  ) {
    return undefined;
  }
  return {
    v: r.v,
    room: r.room,
    key: r.key,
    expires_at_ms: r.expires_at_ms,
    max_viewers: r.max_viewers,
    read_hash: r.read_hash,
    sig: r.sig,
  };
}

/** `RoomRegistration::signing_bytes`: "pm-room-reg-v1" and the six fields, one per line, no trailing newline. */
export function registrationBytes(reg: Registration): Uint8Array {
  return encoder.encode(
    `pm-room-reg-v1\n${reg.v}\n${reg.room}\n${reg.key}\n${reg.expires_at_ms}\n${reg.max_viewers}\n${reg.read_hash}`,
  );
}

/** The first refusal of `RoomRegistration::verify`, in its order, or undefined when the registration is good. */
export async function registrationError(reg: Registration, now: number): Promise<string | undefined> {
  if (reg.v !== 1) return "unsupported room version";
  const key = unb64(reg.key);
  if (key === undefined || key.length !== 32) return "bad room key";
  if (reg.room !== (await roomIdOf(key))) return "room id does not match its key";
  if (reg.expires_at_ms <= now) return "room expiry is in the past";
  if (reg.expires_at_ms > now + ROOM.maxLifetimeMs) return "room lives longer than the relay allows";
  if (reg.max_viewers === 0 || reg.max_viewers > ROOM.maxViewers) return "bad viewer limit";
  const sig = unb64(reg.sig) ?? new Uint8Array(0);
  if (!(await verifyEd25519(key, registrationBytes(reg), sig))) return "bad registration signature";
  return undefined;
}

// ------------------------------------------------------------------------------ Ed25519

/** The group order L = 2^252 + 27742317777372353535851937790883648493. */
const GROUP_ORDER = (1n << 252n) + 27742317777372353535851937790883648493n;

/** Little-endian bytes as an integer. */
function leInteger(bytes: Uint8Array): bigint {
  let n = 0n;
  for (let i = bytes.length - 1; i >= 0; i -= 1) n = (n << 8n) | BigInt(bytes[i]);
  return n;
}

/** The field prime p = 2^255 - 19. */
const FIELD_PRIME = (1n << 255n) - 19n;

/**
 * The y-coordinates of the small-order points of edwards25519: 0 (order 4), 1 (the identity), p - 1 (order 2),
 * and the two values of the eight points of order 8. A point of small order has one of these y-coordinates
 * whatever the sign bit and whether or not the encoding is canonical, so checking y covers every encoding.
 */
const fromHex = (hex: string): Uint8Array => Uint8Array.from(hex.match(/../g) ?? [], (pair) => parseInt(pair, 16));
const SMALL_ORDER_Y = new Set<bigint>([
  0n,
  1n,
  FIELD_PRIME - 1n,
  leInteger(fromHex("c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a")),
  leInteger(fromHex("26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05")),
]);

/** Whether a 32-byte point encoding is of small order (refused by `verify_strict`), canonical or not. */
function isSmallOrder(encoded: Uint8Array): boolean {
  const y = leInteger(encoded) & ((1n << 255n) - 1n);
  return SMALL_ORDER_Y.has(y % FIELD_PRIME);
}

/**
 * Ed25519 by `key` (32 bytes) over `message`, strictly: 64-byte signature, canonical `S`, and neither the key
 * nor `R` small-order. Any failure (bad key point, wrong signature) is `false`; nothing is logged.
 */
export async function verifyEd25519(key: Uint8Array, message: Uint8Array, sig: Uint8Array): Promise<boolean> {
  if (key.length !== 32 || sig.length !== 64) return false;
  if (isSmallOrder(key) || isSmallOrder(sig.subarray(0, 32))) return false;
  if (leInteger(sig.subarray(32)) >= GROUP_ORDER) return false;
  try {
    const publicKey = await crypto.subtle.importKey("raw", key, { name: "Ed25519" }, false, ["verify"]);
    return await crypto.subtle.verify({ name: "Ed25519" }, publicKey, sig, message);
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------------- frames

/** `RoomFrame` (the Rust struct's fields, in its order). */
export interface RoomFrame {
  v: number;
  room: string;
  seq: number;
  kind: "snap" | "delta" | "state";
  nonce: string;
  ct: string;
}

/** The frame as the relay stores and serves it (`serde_json::to_vec` of the Rust struct: same field order). */
export function canonicalFrame(frame: RoomFrame): string {
  return JSON.stringify({ v: frame.v, room: frame.room, seq: frame.seq, kind: frame.kind, nonce: frame.nonce, ct: frame.ct });
}

/** A frame as the Rust type reads it, or undefined (a `bad frame`). */
export function parseFrame(body: Uint8Array): RoomFrame | undefined {
  let raw: unknown;
  try {
    raw = JSON.parse(decoder.decode(body));
  } catch {
    return undefined;
  }
  if (typeof raw !== "object" || raw === null) return undefined;
  const f = raw as Record<string, unknown>;
  if (
    !isUint(f.v, 255) ||
    typeof f.room !== "string" ||
    !isUint(f.seq, Number.MAX_SAFE_INTEGER) ||
    (f.kind !== "snap" && f.kind !== "delta" && f.kind !== "state") ||
    typeof f.nonce !== "string" ||
    typeof f.ct !== "string"
  ) {
    return undefined;
  }
  return { v: f.v, room: f.room, seq: f.seq, kind: f.kind, nonce: f.nonce, ct: f.ct };
}

/** `RoomFrame::check_shape`: the first refusal, in its order, or undefined. No key is needed for this. */
export function frameShapeError(frame: RoomFrame): string | undefined {
  if (frame.v !== 1) return "unsupported frame version";
  if (!isRoomId(frame.room)) return "bad room id";
  if (frame.seq === 0) return "bad sequence number";
  if (unb64(frame.nonce)?.length !== 12) return "bad nonce";
  if (unb64(frame.ct) === undefined) return "bad ciphertext";
  const limit = frame.kind === "snap" ? ROOM.maxSnapshotBytes : ROOM.maxFrameBytes;
  if (encoder.encode(canonicalFrame(frame)).length > limit) return "frame too large";
  return undefined;
}
