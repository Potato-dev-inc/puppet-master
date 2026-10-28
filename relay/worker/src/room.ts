import { DurableObject } from "cloudflare:workers";
import { type Env, limitsFrom } from "./env";
import { empty, json, jsonText, queryU64, Refusal, refuse } from "./http";
import {
  canonicalFrame,
  frameShapeError,
  parseFrame,
  parseRegistration,
  ROOM,
  type RoomFrame,
  sha256,
} from "./roomproto";

/**
 * Live Rooms on the Worker (SPEC-n32 §3.7, §4): one Durable Object per room (`Room`) and one for the
 * relay's open rooms (`RoomIndex`). The Worker (`index.ts`) has checked the body size, the owner's signature
 * (wasm) and the viewer's read token shape; the objects decide everything stateful, in the order of
 * relay/server/src/rooms.rs, so the same tests pass against both.
 *
 * What is kept, and where: the registration (room key, hash of the read token, expiry, viewer limit), the
 * newest snapshot and up to 300 deltas or 2 MiB of them (SQLite, tables `room`, `frames`, `snapshot`). Seats
 * and the owner-signature replay guard are memory only. Frames are ciphertext; the content key never reaches
 * this code. Nothing is logged: no console call in this file.
 *
 * Expiry: an alarm at `expires_at` ends the room (frames gone at once), a second one at `expires_at +` the
 * tombstone wipes the rows. Until then a call answers 410 `{"ended":…}`, then 404 (as the axum relay does).
 */

const encoder = new TextEncoder();
const DAY_MS = 24 * 60 * 60 * 1000;

type End = "stopped" | "expired";

/** 410 with the body the viewer reads: `{"ended":"stopped"}`. */
class Ended extends Refusal {
  constructor(readonly end: End) {
    super(410, "ended");
  }
  override response(): Response {
    return json({ ended: this.end }, 410);
  }
}

/** 429 with `{"full":true}`: the viewer limit (or the seats of one address) is reached. */
class Full extends Refusal {
  constructor() {
    super(429, "full");
  }
  override response(): Response {
    return json({ full: true }, 429);
  }
}

/** The one `room` row of a Room object. Column names are the SQL ones. */
interface RoomRow {
  room: string;
  room_key: string;
  read_hash: string;
  expires_at_ms: number;
  max_viewers: number;
  ended: End | null;
  last_seq: number;
  tokens: number;
  bucket_at_ms: number;
  day_start_ms: number;
  day_count: number;
  delta_count: number;
  delta_bytes: number;
}

/** A frame as the relay keeps it: its sequence number and its canonical JSON. */
interface Stored {
  seq: number;
  body: string;
}

/** `RoomBatch` as JSON text (camelCase: frames, next, ended). */
function batchText(frames: Stored[], next: number, ended: End | null): string {
  const bodies = frames.map((f) => f.body).join(",");
  return `{"frames":[${bodies}],"next":${next},"ended":${ended === null ? "null" : JSON.stringify(ended)}}`;
}

/** Constant-time text equality (the read token's hash is compared without a prefix leak). */
function same(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i += 1) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

export class Room extends DurableObject<Env> {
  private readonly sql: SqlStorage;
  /** Long polls waiting for a frame, or for the room to end. */
  private readonly waiters = new Set<() => void>();
  /** Owner signatures seen lately (memory only; a replay after eviction is harmless: frames need a rising seq). */
  private readonly seen = new Map<string, number>();
  /** Viewer seats: id -> last request and the client address (hashed by the Worker). Never stored. */
  private readonly seats = new Map<string, { last: number; addr: string }>();

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.sql = ctx.storage.sql;
    ctx.blockConcurrencyWhile(async () => this.migrate());
  }

  private migrate(): void {
    this.sql.exec(`CREATE TABLE IF NOT EXISTS room (
      id INTEGER PRIMARY KEY CHECK (id = 1),
      room TEXT NOT NULL,
      room_key TEXT NOT NULL,
      read_hash TEXT NOT NULL,
      expires_at_ms INTEGER NOT NULL,
      max_viewers INTEGER NOT NULL,
      ended TEXT,
      last_seq INTEGER NOT NULL,
      tokens REAL NOT NULL,
      bucket_at_ms INTEGER NOT NULL,
      day_start_ms INTEGER NOT NULL,
      day_count INTEGER NOT NULL,
      delta_count INTEGER NOT NULL,
      delta_bytes INTEGER NOT NULL
    )`);
    this.sql.exec(`CREATE TABLE IF NOT EXISTS frames (
      seq INTEGER PRIMARY KEY,
      body TEXT NOT NULL,
      size INTEGER NOT NULL
    )`);
    this.sql.exec(`CREATE TABLE IF NOT EXISTS snapshot (
      id INTEGER PRIMARY KEY CHECK (id = 1),
      seq INTEGER NOT NULL,
      body TEXT NOT NULL,
      size INTEGER NOT NULL
    )`);
  }

  async fetch(request: Request): Promise<Response> {
    const route = request.headers.get("x-pm-int-route") ?? "";
    try {
      return await this.route(route, request);
    } catch (error) {
      if (error instanceof Refusal) return error.response();
      return refuse(500, "internal error");
    }
  }

  private async route(route: string, request: Request): Promise<Response> {
    const url = new URL(request.url);
    // Bodies are read first, so no other request can change the room between the checks below.
    const hasBody = route === "create" || route === "snapshot-put" || route === "frames-post";
    const body = hasBody ? new Uint8Array(await request.arrayBuffer()) : new Uint8Array(0);
    const now = Date.now();
    switch (route) {
      case "create":
        return this.create(body, now);
      case "status": {
        const row = this.ownerRow(request, now);
        if (row.ended !== null) throw new Ended(row.ended);
        return json(this.status(now, row));
      }
      case "stop": {
        const row = this.ownerRow(request, now);
        if (row.ended !== null) throw new Ended(row.ended);
        this.end("stopped");
        return empty(204);
      }
      case "snapshot-put":
        return this.putSnapshot(request, body, now);
      case "snapshot-get": {
        this.viewerRow(request, now);
        const snap = this.sql.exec("SELECT body FROM snapshot WHERE id = 1").toArray()[0];
        return snap === undefined ? empty(204) : jsonText(String(snap.body));
      }
      case "frames-post":
        return this.postFrame(request, body, now);
      case "frames-get":
        return this.getFrames(request, url, now);
      default:
        return refuse(404, "not found");
    }
  }

  // ------------------------------------------------------------------------------ the room row

  private row(): RoomRow | undefined {
    return this.sql.exec("SELECT * FROM room WHERE id = 1").toArray()[0] as unknown as RoomRow | undefined;
  }

  /** The room as of `now` (it may end now). 404 when there is none, or its tombstone is past. */
  private load(now: number): RoomRow {
    const row = this.row();
    if (row === undefined || now >= row.expires_at_ms + ROOM.tombstoneMs) throw new Refusal(404, "not found");
    if (row.ended === null && now >= row.expires_at_ms) {
      this.end("expired");
      return { ...row, ended: "expired" };
    }
    return row;
  }

  /** Ends the room: frames, snapshot and seats go at once; the row stays as the tombstone. */
  private end(kind: End): void {
    this.sql.exec("UPDATE room SET ended = ?, delta_count = 0, delta_bytes = 0 WHERE id = 1", kind);
    this.sql.exec("DELETE FROM frames");
    this.sql.exec("DELETE FROM snapshot");
    this.seats.clear();
    this.wake();
  }

  private async create(body: Uint8Array, now: number): Promise<Response> {
    const reg = parseRegistration(body);
    if (reg === undefined) throw new Refusal(400, "bad registration");
    const old = this.row();
    // A used id is refused while its tombstone lasts, so a stopped room's id is not reused at once.
    if (old !== undefined && now < old.expires_at_ms + ROOM.tombstoneMs) throw new Refusal(409, "room already exists");
    this.sql.exec("DELETE FROM frames");
    this.sql.exec("DELETE FROM snapshot");
    this.sql.exec("DELETE FROM room");
    this.sql.exec(
      `INSERT INTO room (id, room, room_key, read_hash, expires_at_ms, max_viewers, ended, last_seq, tokens,
        bucket_at_ms, day_start_ms, day_count, delta_count, delta_bytes)
       VALUES (1, ?, ?, ?, ?, ?, NULL, 0, ?, ?, ?, 0, 0, 0)`,
      reg.room,
      reg.key,
      reg.read_hash,
      reg.expires_at_ms,
      reg.max_viewers,
      ROOM.frameBurst,
      now,
      now,
    );
    this.seats.clear();
    this.seen.clear();
    await this.ctx.storage.setAlarm(reg.expires_at_ms);
    return empty(201);
  }

  // ------------------------------------------------------------------------------ owner calls

  /** The room, after the owner's checks: its key signed the request, and the signature was not seen before. */
  private ownerRow(request: Request, now: number): RoomRow {
    const row = this.load(now);
    const device = request.headers.get("x-pm-int-device");
    const sig = request.headers.get("x-pm-int-sig");
    if (!device || !sig) throw new Refusal(401, "signed request required");
    if (device !== row.room_key) throw new Refusal(403, "not the room's key");
    for (const [old, until] of this.seen) if (until <= now) this.seen.delete(old);
    if (this.seen.has(sig)) throw new Refusal(401, "request replayed");
    this.seen.set(sig, now + ROOM.seenForMs);
    return row;
  }

  /** `RoomStatus`: two numbers. */
  private status(now: number, row: RoomRow): { viewers: number; expiresAtMs: number } {
    let viewers = 0;
    for (const seat of this.seats.values()) if (now - seat.last < ROOM.seatActiveMs) viewers += 1;
    return { viewers, expiresAtMs: row.expires_at_ms };
  }

  /** The checks of `check_frame` and then the owner's budget, before a frame is kept. */
  private admit(row: RoomRow, seq: number, now: number): void {
    if (row.ended !== null) throw new Ended(row.ended);
    if (seq <= row.last_seq) throw new Refusal(409, "frame sequence number does not rise");
    this.spend(row, now);
  }

  /** One frame from the room's budget: a token bucket (burst 10, 2 a second) and a daily count. */
  private spend(row: RoomRow, now: number): void {
    let tokens = Math.min(ROOM.frameBurst, row.tokens + (Math.max(0, now - row.bucket_at_ms) / 1000) * ROOM.frameRefillPerSecond);
    let dayStart = row.day_start_ms;
    let dayCount = row.day_count;
    if (now - dayStart >= DAY_MS) {
      dayStart = now;
      dayCount = 0;
    }
    const allowed = tokens >= 1 && dayCount < ROOM.framesPerDay;
    if (allowed) {
      tokens -= 1;
      dayCount += 1;
    }
    this.sql.exec(
      "UPDATE room SET tokens = ?, bucket_at_ms = ?, day_start_ms = ?, day_count = ? WHERE id = 1",
      tokens,
      now,
      dayStart,
      dayCount,
    );
    if (!allowed) throw new Refusal(429, "slow down");
  }

  private putSnapshot(request: Request, body: Uint8Array, now: number): Response {
    const row = this.ownerRow(request, now);
    const frame = parseFrame(body);
    if (frame === undefined) throw new Refusal(400, "bad frame");
    if (frame.kind !== "snap") throw new Refusal(400, "a snapshot must be kind snap");
    const why = frameShapeError(frame);
    if (why !== undefined) throw new Refusal(400, why);
    if (frame.room !== row.room) throw new Refusal(400, "the frame is for another room");
    this.admit(row, frame.seq, now);
    // A snapshot replaces the snapshot and every delta (all of them are older: seq rises).
    this.sql.exec("DELETE FROM frames");
    this.sql.exec(
      "INSERT OR REPLACE INTO snapshot (id, seq, body, size) VALUES (1, ?, ?, ?)",
      frame.seq,
      canonicalFrame(frame),
      body.byteLength,
    );
    this.sql.exec("UPDATE room SET last_seq = ?, delta_count = 0, delta_bytes = 0 WHERE id = 1", frame.seq);
    this.wake();
    return json(this.status(now, row));
  }

  private postFrame(request: Request, body: Uint8Array, now: number): Response {
    const row = this.ownerRow(request, now);
    const frame = parseFrame(body);
    if (frame === undefined) throw new Refusal(400, "bad frame");
    if (frame.kind === "snap") throw new Refusal(400, "frames are kind delta or state");
    const why = frameShapeError(frame);
    if (why !== undefined) throw new Refusal(400, why);
    if (frame.room !== row.room) throw new Refusal(400, "the frame is for another room");
    this.admit(row, frame.seq, now);
    this.keepDelta(frame, canonicalFrame(frame), body.byteLength);
    this.wake();
    return json(this.status(now, row));
  }

  /** Keeps a delta: the oldest go while the ring is over its frame or byte cap. */
  private keepDelta(frame: RoomFrame, text: string, size: number): void {
    this.sql.exec("INSERT INTO frames (seq, body, size) VALUES (?, ?, ?)", frame.seq, text, size);
    const row = this.row() as RoomRow;
    let count = row.delta_count + 1;
    let bytes = row.delta_bytes + size;
    this.sql.exec("UPDATE room SET last_seq = ?, delta_count = ?, delta_bytes = ? WHERE id = 1", frame.seq, count, bytes);
    while (count > ROOM.ringFrames || bytes > ROOM.ringBytes) {
      const oldest = this.sql.exec("SELECT seq, size FROM frames ORDER BY seq LIMIT 1").one();
      this.sql.exec("DELETE FROM frames WHERE seq = ?", oldest.seq);
      count -= 1;
      bytes -= Number(oldest.size);
      this.sql.exec("UPDATE room SET delta_count = ?, delta_bytes = ? WHERE id = 1", count, bytes);
    }
  }

  // ----------------------------------------------------------------------------- viewer calls

  /**
   * A viewer's call: the read token must open the room, the room must be live, and the seat must have room.
   * A token that does not match is the same 404 as an unknown room.
   */
  private viewerRow(request: Request, now: number): RoomRow {
    const row = this.load(now);
    const read = request.headers.get("x-pm-int-read") ?? "";
    if (!same(read, row.read_hash)) throw new Refusal(404, "not found");
    if (row.ended !== null) throw new Ended(row.ended);
    this.seat(request.headers.get("x-pm-int-seat") ?? "", request.headers.get("x-pm-int-addr") ?? "", row, now);
    return row;
  }

  /** Refreshes an active seat, or takes a new one when the room and the address have room. */
  private seat(id: string, addr: string, row: RoomRow, now: number): void {
    const known = this.seats.get(id);
    if (known !== undefined && now - known.last < ROOM.seatActiveMs) {
      known.last = now;
      return;
    }
    for (const [other, seat] of this.seats) if (now - seat.last >= ROOM.seatActiveMs) this.seats.delete(other);
    if (this.seats.size >= row.max_viewers) throw new Full();
    let fromAddress = 0;
    for (const seat of this.seats.values()) if (seat.addr === addr) fromAddress += 1;
    if (fromAddress >= ROOM.seatsPerAddress) throw new Full();
    this.seats.set(id, { last: now, addr });
  }

  /** Frames a viewer that has seen `after` still needs: the snapshot if it is newer, then the deltas. */
  private framesAfter(after: number): Stored[] {
    const out: Stored[] = [];
    const snap = this.sql.exec("SELECT seq, body FROM snapshot WHERE id = 1 AND seq > ?", after).toArray()[0];
    if (snap !== undefined) out.push({ seq: Number(snap.seq), body: String(snap.body) });
    const left = ROOM.batchFrames - out.length;
    for (const row of this.sql.exec("SELECT seq, body FROM frames WHERE seq > ? ORDER BY seq LIMIT ?", after, left).toArray()) {
      out.push({ seq: Number(row.seq), body: String(row.body) });
    }
    return out;
  }

  /** `GET …/frames?after=N&wait=S`: answers at once with frames, or waits up to `wait` seconds for some. */
  private async getFrames(request: Request, url: URL, now: number): Promise<Response> {
    const after = queryU64(url, "after") ?? 0;
    const wait = Math.min(queryU64(url, "wait") ?? 0, limitsFrom(this.env).maxWaitS);
    const deadline = now + wait * 1000;
    let waited = false;
    for (;;) {
      let frames: Stored[];
      try {
        this.viewerRow(request, Date.now());
        frames = this.framesAfter(after);
      } catch (error) {
        // The room ended while this call waited: the answer says so, and the viewer stops.
        if (waited && error instanceof Ended) return jsonText(batchText([], after, error.end));
        throw error;
      }
      const left = deadline - Date.now();
      if (frames.length > 0 || left <= 0) {
        const next = frames.length > 0 ? frames[frames.length - 1].seq : after;
        return jsonText(batchText(frames, next, null));
      }
      waited = true;
      await this.waitForChange(left);
    }
  }

  /** Resolves when a frame lands or the room ends, or after `ms`. */
  private waitForChange(ms: number): Promise<void> {
    return new Promise((resolve) => {
      const done = () => {
        clearTimeout(timer);
        this.waiters.delete(done);
        resolve();
      };
      const timer = setTimeout(done, Math.max(ms, 0));
      this.waiters.add(done);
    });
  }

  private wake(): void {
    for (const waiter of [...this.waiters]) waiter();
  }

  // ------------------------------------------------------------------------------- the alarm

  /** Ends the room at its expiry, then wipes its rows once the tombstone is over. */
  async alarm(): Promise<void> {
    const row = this.row();
    if (row === undefined) return;
    const now = Date.now();
    if (now < row.expires_at_ms) {
      await this.ctx.storage.setAlarm(row.expires_at_ms);
      return;
    }
    if (now >= row.expires_at_ms + ROOM.tombstoneMs) {
      // Rows are emptied, not dropped: the tables stay for the next room this object holds.
      this.sql.exec("DELETE FROM frames");
      this.sql.exec("DELETE FROM snapshot");
      this.sql.exec("DELETE FROM room");
      this.seats.clear();
      this.seen.clear();
      return;
    }
    if (row.ended === null) this.end("expired");
    await this.ctx.storage.setAlarm(row.expires_at_ms + ROOM.tombstoneMs);
  }
}

/**
 * The relay's open rooms: one object for the whole relay, so the cap (`max_open_rooms`, 100) is exact.
 * A row leaves when its room is stopped (`release`) or its time is up (pruned at the next reserve).
 */
export class RoomIndex extends DurableObject<Env> {
  private readonly sql: SqlStorage;

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.sql = ctx.storage.sql;
    ctx.blockConcurrencyWhile(async () => {
      this.sql.exec("CREATE TABLE IF NOT EXISTS open_rooms (room TEXT PRIMARY KEY, expires_at_ms INTEGER NOT NULL)");
    });
  }

  /** A slot for a new room: `ok`, `exists` (an open room has the id) or `busy` (the cap is reached). */
  async reserve(room: string, expiresAtMs: number, max: number, now: number): Promise<"ok" | "exists" | "busy"> {
    this.sql.exec("DELETE FROM open_rooms WHERE expires_at_ms <= ?", now);
    if (this.sql.exec("SELECT room FROM open_rooms WHERE room = ?", room).toArray().length > 0) return "exists";
    if (Number(this.sql.exec("SELECT COUNT(*) AS n FROM open_rooms").one().n) >= max) return "busy";
    this.sql.exec("INSERT INTO open_rooms (room, expires_at_ms) VALUES (?, ?)", room, expiresAtMs);
    return "ok";
  }

  async release(room: string): Promise<void> {
    this.sql.exec("DELETE FROM open_rooms WHERE room = ?", room);
  }
}

/** A name for a per-address counter (the Worker keeps no address in clear): SHA-256 of the scope and the address. */
export async function scopedKey(scope: string, address: string): Promise<string> {
  const digest = await sha256(encoder.encode(`pm-relay-room-${scope}-v1\0${address}`));
  return [...digest].map((b) => b.toString(16).padStart(2, "0")).join("");
}
