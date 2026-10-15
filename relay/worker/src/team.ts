import { DurableObject } from "cloudflare:workers";
import { DEFAULT_LIMITS, type Env, type Limits, limitsFrom } from "./env";
import { empty, json, jsonText, queryU64, Refusal, refuse } from "./http";
import { batch, C, checkAcl, checkBlob, checkEnvelope, checkJoin, checkRegistration, resync } from "./proto";

/**
 * One Durable Object per team: the mailbox, the access list, the two signed state slots, the join
 * requests, the replay guard and the rate limits of relay/server, in the object's own SQLite store.
 * Open WebSockets use the hibernation API, so an idle team costs nothing between messages.
 *
 * The Worker (`index.ts`) has already checked the request's signature; it tells this object the
 * route and the verified device in `x-pm-int-*` headers (a Durable Object is not reachable from
 * outside the Worker, and the Worker overwrites those headers). Everything stateful is decided
 * here: team exists, device on the access list, request not replayed, limits, storage.
 *
 * It mirrors relay/server (the hardened one) rule for rule, so the same end-to-end tests pass
 * against both (relay/server/tests/protocol.rs with PM_RELAY_TEST_URL). The mailbox position
 * (`seq`) comes from a counter that only grows (`team.next_seq`, so the team's newest position is
 * `head = next_seq - 1`); a device reads only after its join point (`devices.from_seq`), the
 * position the team was at when the device was added to the access list.
 */

/** How often the sweeper wakes while anything can still expire. */
const SWEEP_EVERY_MS = 60 * 60 * 1000;
/** Replay guard memory: a signed request is valid for `AUTH_SKEW_MS` either side of now. */
const SEEN_FOR_MS = 2 * C.authSkewMs + 1_000;
const HOUR_MS = 60 * 60 * 1000;
const DAY_MS = 24 * HOUR_MS;
/** Envelopes per batch. */
const BATCH = 200;
/** Largest WebSocket message a client may send (clients only listen; this is for pings). */
const MAX_CLIENT_FRAME = 4 * 1024;
/** Prune expired rate-limit windows after this many counted hits. */
const PRUNE_EVERY_HITS = 200;

const encoder = new TextEncoder();

interface Meta {
  team: string;
  root: string;
  aclVersion: number;
}

interface Row {
  seq: number;
  at_ms: number;
  body: string;
}

interface Attachment {
  device: string;
  cursor: number;
}

/** A device request that passed the access list, the replay guard and the per-device cap. */
interface Authed {
  device: string;
  /** The mailbox position the device was added at: it reads only after it. */
  fromSeq: number;
}

/** What a read of the mailbox returns (relay/protocol `Batch`, before it is serialised). */
interface Page {
  rows: Row[];
  next: number;
  head: number;
  reset: boolean;
}

export class Team extends DurableObject<Env> {
  private readonly sql: SqlStorage;
  private metaCache: Meta | undefined;
  /** The access list as `device -> join point`, read from the `devices` table. */
  private membersCache: Map<string, number> | undefined;
  /** Long polls waiting for mail (or for an access-list change). */
  private readonly waiters = new Set<() => void>();
  private armed = false;
  private hitsSincePrune = 0;
  /** Test hook: limits overridden for this team (only read when TEST_HOOKS is on). */
  private override: Partial<Limits> = {};
  private limitsCache: Limits | undefined;

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.sql = ctx.storage.sql;
    ctx.blockConcurrencyWhile(async () => {
      this.migrate();
      if (env.TEST_HOOKS === "1") {
        const saved = this.sql.exec("SELECT value FROM kv WHERE key = 'limits'").toArray()[0];
        if (saved) this.override = JSON.parse(String(saved.value)) as Partial<Limits>;
      }
    });
  }

  private migrate(): void {
    this.sql.exec(`
      CREATE TABLE IF NOT EXISTS team (id INTEGER PRIMARY KEY CHECK (id = 1), team TEXT NOT NULL, root TEXT NOT NULL,
        salt TEXT NOT NULL, acl_version INTEGER NOT NULL, acl TEXT NOT NULL, created_ms INTEGER NOT NULL,
        next_seq INTEGER NOT NULL);
      CREATE TABLE IF NOT EXISTS state (slot TEXT PRIMARY KEY, version INTEGER NOT NULL, blob TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS envelopes (seq INTEGER PRIMARY KEY, sender TEXT NOT NULL, at_ms INTEGER NOT NULL,
        body TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS joins (id INTEGER PRIMARY KEY AUTOINCREMENT, invite TEXT NOT NULL,
        at_ms INTEGER NOT NULL, body TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS windows (key TEXT PRIMARY KEY, start_ms INTEGER NOT NULL, span_ms INTEGER NOT NULL,
        count INTEGER NOT NULL);
      CREATE TABLE IF NOT EXISTS seen (sig TEXT PRIMARY KEY, until_ms INTEGER NOT NULL);
      CREATE INDEX IF NOT EXISTS seen_until ON seen (until_ms);
      CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS devices (device TEXT PRIMARY KEY, from_seq INTEGER NOT NULL);
    `);
    // Objects from before the storage quota: add the counters and fill them from the rows once.
    const columns = this.sql.exec("PRAGMA table_info(team)").toArray().map((r) => String(r.name));
    let counted = true;
    for (const column of ["stored_rows", "stored_bytes"]) {
      if (!columns.includes(column)) {
        this.sql.exec(`ALTER TABLE team ADD COLUMN ${column} INTEGER NOT NULL DEFAULT 0`);
        counted = false;
      }
    }
    if (!counted) this.recount();
    // Objects from before join points: every listed device keeps reading the whole mailbox, as before.
    if (!this.sql.exec("SELECT 1 FROM kv WHERE key = 'join-points'").toArray()[0]) {
      const row = this.sql.exec("SELECT acl FROM team WHERE id = 1").toArray()[0];
      if (row) {
        for (const device of JSON.parse(String(row.acl)) as string[]) {
          this.sql.exec("INSERT OR IGNORE INTO devices (device, from_seq) VALUES (?, 0)", device);
        }
      }
      this.sql.exec("INSERT OR REPLACE INTO kv (key, value) VALUES ('join-points', '1')");
    }
  }

  /** The stored envelope count and bytes, from the rows themselves (after a sweep or a dropped row). */
  private recount(): void {
    this.sql.exec(
      `UPDATE team SET
         stored_rows = (SELECT COUNT(*) FROM envelopes),
         stored_bytes = (SELECT COALESCE(SUM(LENGTH(CAST(body AS BLOB))), 0) FROM envelopes)`,
    );
  }

  // ------------------------------------------------------------------------------- state helpers

  private meta(): Meta | undefined {
    if (!this.metaCache) {
      const row = this.sql.exec("SELECT team, root, acl_version FROM team WHERE id = 1").toArray()[0];
      if (row) this.metaCache = { team: String(row.team), root: String(row.root), aclVersion: Number(row.acl_version) };
    }
    return this.metaCache;
  }

  private requireTeam(): Meta {
    const meta = this.meta();
    if (!meta) throw new Refusal(404, "unknown team");
    return meta;
  }

  /** The access list with each device's join point. */
  private members(): Map<string, number> {
    if (!this.membersCache) {
      const rows = this.sql.exec("SELECT device, from_seq FROM devices").toArray();
      this.membersCache = new Map(rows.map((r) => [String(r.device), Number(r.from_seq)]));
    }
    return this.membersCache;
  }

  /** The team's newest mailbox position. */
  private head(): number {
    const row = this.sql.exec("SELECT next_seq FROM team WHERE id = 1").toArray()[0];
    return row ? Number(row.next_seq) - 1 : 0;
  }

  private limits(): Limits {
    this.limitsCache ??= { ...limitsFrom(this.env), ...this.override };
    return this.limitsCache;
  }

  /** Count one more in a window of `spanMs`; false when over `max`. */
  private hit(key: string, spanMs: number, max: number): boolean {
    const now = Date.now();
    if (++this.hitsSincePrune >= PRUNE_EVERY_HITS) {
      this.hitsSincePrune = 0;
      this.sql.exec("DELETE FROM windows WHERE start_ms + span_ms <= ?", now);
    }
    const row = this.sql.exec("SELECT start_ms, count FROM windows WHERE key = ?", key).toArray()[0];
    let start = now;
    let count = 1;
    if (row && now - Number(row.start_ms) < spanMs) {
      start = Number(row.start_ms);
      count = Number(row.count) + 1;
    }
    this.sql.exec(
      `INSERT INTO windows (key, start_ms, span_ms, count) VALUES (?, ?, ?, ?)
       ON CONFLICT (key) DO UPDATE SET start_ms = excluded.start_ms, span_ms = excluded.span_ms, count = excluded.count`,
      key,
      start,
      spanMs,
      count,
    );
    return count <= max;
  }

  /**
   * A signed request from a device on the team's access list that was not seen before, and is within the
   * device's request cap. In this order, as relay/server does: the access list, the replay guard (so
   * strangers' requests are never remembered), the cap.
   */
  private authorize(request: Request): Authed {
    const device = request.headers.get("x-pm-int-device");
    const sig = request.headers.get("x-pm-int-sig");
    if (!device || !sig) throw new Refusal(401, "signed request required");
    this.requireTeam();
    const fromSeq = this.members().get(device);
    if (fromSeq === undefined) throw new Refusal(403, "device not on this team");
    // Only a device on the access list gets here, so the replay guard holds signatures of team
    // members only. Expired rows go on every call (an index range delete, free when nothing expired).
    const now = Date.now();
    this.sql.exec("DELETE FROM seen WHERE until_ms <= ?", now);
    const seen = this.sql.exec("SELECT until_ms FROM seen WHERE sig = ?", sig).toArray()[0];
    if (seen && Number(seen.until_ms) > now) throw new Refusal(401, "request replayed");
    this.sql.exec(
      "INSERT INTO seen (sig, until_ms) VALUES (?, ?) ON CONFLICT (sig) DO UPDATE SET until_ms = excluded.until_ms",
      sig,
      now + SEEN_FOR_MS,
    );
    if (!this.hit(`dev-req:${device}`, 60_000, this.limits().deviceRequestsPerMinute)) throw new Refusal(429, "slow down");
    return { device, fromSeq };
  }

  // ------------------------------------------------------------------------------------- routing

  async fetch(request: Request): Promise<Response> {
    const route = request.headers.get("x-pm-int-route") ?? "";
    try {
      const response = await this.route(route, request);
      if (route !== "register" && !route.startsWith("test-")) await this.arm();
      return response;
    } catch (error) {
      if (error instanceof Refusal) return error.response();
      console.warn("relay: internal error", error);
      return refuse(500, "internal error");
    }
  }

  private async route(route: string, request: Request): Promise<Response> {
    const url = new URL(request.url);
    const slot = request.headers.get("x-pm-int-slot") ?? "";
    switch (route) {
      case "register":
        return this.register(request, new Uint8Array(await request.arrayBuffer()));
      case "acl":
        return this.setAcl(new Uint8Array(await request.arrayBuffer()));
      case "state-put":
        return this.putState(request, slot, new Uint8Array(await request.arrayBuffer()));
      case "state-get":
        return this.getState(request, slot);
      case "env-post":
        return this.postEnvelope(request, new Uint8Array(await request.arrayBuffer()));
      case "env-get":
        return this.getEnvelopes(request, url);
      case "ws":
        return this.openSocket(request, url);
      case "join-post":
        return this.postJoin(request, new Uint8Array(await request.arrayBuffer()));
      case "join-get":
        return this.getJoins(request);
      default:
        if (route.startsWith("test-") && this.env.TEST_HOOKS === "1") return this.testHook(route, request, url);
        return refuse(404, "not found");
    }
  }

  // ---------------------------------------------------------------------------------------- team

  private register(request: Request, body: Uint8Array): Response {
    const verdict = checkRegistration(body);
    if (!verdict.ok) return refuse(verdict.status, verdict.error);
    if (verdict.team !== request.headers.get("x-pm-int-team")) return refuse(400, "bad registration");
    if (this.meta()) return refuse(409, "team already registered");
    this.ctx.storage.transactionSync(() => {
      this.sql.exec(
        `INSERT INTO team (id, team, root, salt, acl_version, acl, created_ms, next_seq, stored_rows, stored_bytes)
         VALUES (1, ?, ?, ?, ?, ?, ?, 1, 0, 0)`,
        verdict.team,
        verdict.root,
        verdict.salt,
        verdict.version,
        JSON.stringify(verdict.devices),
        Date.now(),
      );
      // Everyone listed at registration reads from the start.
      for (const device of verdict.devices) {
        this.sql.exec("INSERT OR IGNORE INTO devices (device, from_seq) VALUES (?, 0)", device);
      }
    });
    this.metaCache = undefined;
    this.membersCache = undefined;
    console.log("relay: team registered");
    return empty(201);
  }

  private setAcl(body: Uint8Array): Response {
    const meta = this.requireTeam();
    const verdict = checkAcl(body, meta.team, meta.root);
    if (!verdict.ok) return refuse(verdict.status, verdict.error);
    if (verdict.version <= meta.aclVersion) {
      return verdict.version === meta.aclVersion ? empty(204) : refuse(409, "older access list");
    }
    this.ctx.storage.transactionSync(() => {
      const head = this.head();
      this.sql.exec("UPDATE team SET acl = ?, acl_version = ? WHERE id = 1", JSON.stringify(verdict.devices), verdict.version);
      const listed = new Set(verdict.devices);
      for (const row of this.sql.exec("SELECT device FROM devices").toArray()) {
        if (!listed.has(String(row.device))) this.sql.exec("DELETE FROM devices WHERE device = ?", String(row.device));
      }
      // A device new to the list reads from here on: not what the team said before it joined.
      for (const device of listed) {
        this.sql.exec("INSERT OR IGNORE INTO devices (device, from_seq) VALUES (?, ?)", device, head);
      }
    });
    this.metaCache = undefined;
    this.membersCache = undefined;
    // A device taken off the list loses its open socket and its waiting long poll too, not only new requests.
    this.closeRemoved();
    this.wake();
    return empty(204);
  }

  // ----------------------------------------------------------------------------------- state slots

  private putState(request: Request, slot: string, body: Uint8Array): Response {
    this.authorize(request);
    const meta = this.requireTeam();
    const verdict = checkBlob(body, meta.team, slot, meta.root);
    if (!verdict.ok) return refuse(verdict.status, verdict.error);
    const current = this.sql.exec("SELECT version FROM state WHERE slot = ?", slot).toArray()[0];
    if (current && Number(current.version) > verdict.version) return refuse(409, "older state");
    this.sql.exec(
      `INSERT INTO state (slot, version, blob) VALUES (?, ?, ?)
       ON CONFLICT (slot) DO UPDATE SET version = excluded.version, blob = excluded.blob`,
      slot,
      verdict.version,
      new TextDecoder().decode(body),
    );
    return empty(204);
  }

  private getState(request: Request, slot: string): Response {
    this.authorize(request);
    if (!C.slots.includes(slot)) return empty(204);
    const row = this.sql.exec("SELECT blob FROM state WHERE slot = ?", slot).toArray()[0];
    return row ? jsonText(String(row.blob)) : empty(204);
  }

  // --------------------------------------------------------------------------------------- mailbox

  private postEnvelope(request: Request, body: Uint8Array): Response {
    const { device } = this.authorize(request);
    const meta = this.requireTeam();
    const verdict = checkEnvelope(body, meta.team, device);
    if (!verdict.ok) return refuse(verdict.status, verdict.error);
    const limits = this.limits();
    if (
      !this.hit(`dev-min:${device}`, 60_000, limits.devicePerMinute) ||
      !this.hit(`dev-day:${device}`, DAY_MS, limits.devicePerDay) ||
      !this.hit(`team-min:${meta.team}`, 60_000, limits.teamPerMinute)
    ) {
      return refuse(429, "slow down");
    }
    const size = encoder.encode(verdict.envelope).byteLength;
    const counters = this.sql.exec("SELECT next_seq, stored_rows, stored_bytes FROM team WHERE id = 1").one();
    const next = Number(counters.next_seq);
    if (Number(counters.stored_rows) >= limits.teamMaxEnvelopes || Number(counters.stored_bytes) + size > limits.teamMaxBytes) {
      return refuse(507, "the team's mailbox is full");
    }
    // The counter only grows, so a cursor never meets a number it has already passed, even after
    // every old envelope has expired.
    const now = Date.now();
    this.ctx.storage.transactionSync(() => {
      this.sql.exec("INSERT INTO envelopes (seq, sender, at_ms, body) VALUES (?, ?, ?, ?)", next, device, now, verdict.envelope);
      this.sql.exec(
        "UPDATE team SET next_seq = ?, stored_rows = stored_rows + 1, stored_bytes = stored_bytes + ? WHERE id = 1",
        next + 1,
        size,
      );
    });
    this.broadcast({ seq: next, at_ms: now, body: verdict.envelope });
    this.wake();
    return json({ seq: next });
  }

  /**
   * Envelopes after `after` that a device with join point `fromSeq` may read. When `after` is past the
   * team's newest position the relay's counter is behind the client: start again from its join point and
   * say so (`reset`). Without items the cursor moves to where this device can read from: after a reset
   * that is the head (nothing is newer), never back below the join point.
   */
  private page(after: number, fromSeq: number, limit: number): Page {
    const head = this.head();
    const reset = after > head;
    const start = reset ? fromSeq : Math.max(after, fromSeq);
    const oldest = Date.now() - C.mailboxTtlMs;
    const rows = this.sql
      .exec("SELECT seq, at_ms, body FROM envelopes WHERE seq > ? AND at_ms >= ? ORDER BY seq LIMIT ?", start, oldest, limit)
      .toArray()
      .map((r) => ({ seq: Number(r.seq), at_ms: Number(r.at_ms), body: String(r.body) }));
    const next = rows.length > 0 ? rows[rows.length - 1].seq : reset ? Math.max(head, fromSeq) : start;
    return { rows, next, head, reset };
  }

  /** What the relay hands out: the envelope with its place in the mailbox (`Delivered`). */
  private delivered(row: Row): string {
    return `{"seq":${row.seq},"at_ms":${row.at_ms},"envelope":${row.body}}`;
  }

  private async getEnvelopes(request: Request, url: URL): Promise<Response> {
    const { device } = this.authorize(request);
    const after = queryU64(url, "after") ?? 0;
    const wait = Math.min(queryU64(url, "wait") ?? 0, this.limits().maxWaitS);
    const deadline = Date.now() + wait * 1000;
    for (;;) {
      // Read the join point afresh: the device may have been removed (and added again) while it waited.
      const fromSeq = this.members().get(device);
      if (fromSeq === undefined) throw new Refusal(403, "device not on this team");
      const page = this.page(after, fromSeq, BATCH);
      if (page.rows.length > 0 || page.reset || Date.now() >= deadline) {
        return jsonText(batch(page.rows.map((r) => this.delivered(r)).join(","), page.next, page.head, page.reset));
      }
      await this.waitForMail(deadline - Date.now());
    }
  }

  /** Resolve when mail arrives for this team (or its access list changes) or after `ms`. */
  private waitForMail(ms: number): Promise<void> {
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

  // -------------------------------------------------------------------------------------- sockets

  private openSocket(request: Request, url: URL): Response {
    const { device } = this.authorize(request);
    if (request.headers.get("upgrade")?.toLowerCase() !== "websocket") return refuse(426, "this is a WebSocket address");
    const after = queryU64(url, "after") ?? 0;
    // A device may keep a few sockets; beyond that the new one is refused.
    // One that is closing (this side has closed it, the client has not answered yet) no longer counts.
    const open = this.ctx
      .getWebSockets()
      .filter((ws) => ws.readyState === WebSocket.OPEN && (ws.deserializeAttachment() as Attachment | null)?.device === device).length;
    if (open >= this.limits().socketsPerDevice) throw new Refusal(429, "too many open connections for this device");
    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair) as [WebSocket, WebSocket];
    this.ctx.acceptWebSocket(server);
    server.serializeAttachment({ device, cursor: after } satisfies Attachment);
    this.catchUp(server); // the backlog; live envelopes are broadcast
    return new Response(null, { status: 101, webSocket: client });
  }

  /**
   * Send one socket everything after its cursor that its device may read (the backlog, or what it
   * missed). A cursor past the team's newest position gets `{"reset":true,"head":N}` first, then the
   * mailbox from the device's join point.
   */
  private catchUp(ws: WebSocket): void {
    const attachment = ws.deserializeAttachment() as Attachment | null;
    if (!attachment) return;
    const fromSeq = this.members().get(attachment.device);
    if (fromSeq === undefined) return;
    try {
      let cursor = attachment.cursor;
      for (;;) {
        const page = this.page(cursor, fromSeq, BATCH);
        if (page.reset) ws.send(resync(page.head));
        for (const row of page.rows) ws.send(this.delivered(row));
        cursor = page.next;
        if (page.rows.length < BATCH) break;
      }
      if (cursor !== attachment.cursor) ws.serializeAttachment({ device: attachment.device, cursor } satisfies Attachment);
    } catch (error) {
      console.debug("relay: socket send failed", error);
    }
  }

  /**
   * A new envelope goes to every open socket from the one message built here: no query per socket.
   * A socket that is behind (it missed one, or one was dropped) catches up with its own query.
   */
  private broadcast(row: Row): void {
    const text = this.delivered(row);
    const allowed = this.members();
    for (const ws of this.ctx.getWebSockets()) {
      const attachment = ws.deserializeAttachment() as Attachment | null;
      if (!attachment) continue;
      try {
        if (!allowed.has(attachment.device)) {
          ws.close(1008, "device not on this team");
        } else if (attachment.cursor === row.seq - 1) {
          ws.send(text);
          ws.serializeAttachment({ device: attachment.device, cursor: row.seq } satisfies Attachment);
        } else if (attachment.cursor < row.seq) {
          this.catchUp(ws);
        }
      } catch (error) {
        console.debug("relay: socket send failed", error);
      }
    }
  }

  /** After an access-list change: close the sockets of devices that are no longer on it. */
  private closeRemoved(): void {
    const allowed = this.members();
    for (const ws of this.ctx.getWebSockets()) {
      const attachment = ws.deserializeAttachment() as Attachment | null;
      if (!attachment || allowed.has(attachment.device)) continue;
      try {
        ws.close(1008, "device not on this team");
      } catch {
        // already closed
      }
    }
  }

  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
    // Clients only listen; anything they say is ignored, and a big message closes the socket
    // instead of being handled.
    const size = typeof message === "string" ? encoder.encode(message).byteLength : message.byteLength;
    if (size > MAX_CLIENT_FRAME) {
      try {
        ws.close(1009, "message too big");
      } catch {
        // already closed
      }
    }
  }

  async webSocketClose(ws: WebSocket, code: number): Promise<void> {
    try {
      ws.close(code >= 1000 && code < 1005 ? code : 1000, "closing");
    } catch {
      // already closed
    }
  }

  async webSocketError(ws: WebSocket): Promise<void> {
    try {
      ws.close(1011, "error");
    } catch {
      // already closed
    }
  }

  // ----------------------------------------------------------------------------------------- joins

  private postJoin(request: Request, body: Uint8Array): Response {
    const meta = this.requireTeam();
    const verdict = checkJoin(body, meta.team);
    if (!verdict.ok) return refuse(verdict.status, verdict.error);
    const limits = this.limits();
    // The address is the front door's hash of the client's (IPv6: its /64), never the address itself.
    const address = request.headers.get("x-pm-int-addr") ?? "unknown";
    // Per address first, then the team's daily cap, then the invite: a flood from one address can't spend
    // the team's cap, and once the team's cap is spent no new invite ids are counted. An invite's attempts
    // are counted for a join request's lifetime, then forgotten.
    if (
      !this.hit(`join-ip:${meta.team}:${address}`, HOUR_MS, limits.joinsPerAddressPerHour) ||
      !this.hit("joins-day", DAY_MS, limits.joinsPerTeamPerDay) ||
      !this.hit(`invite:${verdict.invite}`, C.joinTtlMs, limits.joinsPerInvite)
    ) {
      return refuse(429, "too many join requests");
    }
    this.sql.exec("INSERT INTO joins (invite, at_ms, body) VALUES (?, ?, ?)", verdict.invite, Date.now(), verdict.join);
    return empty(202);
  }

  private getJoins(request: Request): Response {
    this.authorize(request);
    const oldest = Date.now() - C.joinTtlMs;
    const bodies = this.sql
      .exec("SELECT body FROM joins WHERE at_ms >= ? ORDER BY id", oldest)
      .toArray()
      .map((r) => String(r.body));
    return jsonText(`[${bodies.join(",")}]`);
  }

  // ----------------------------------------------------------------------------------- expiry

  /** Make sure a sweep is scheduled. */
  private async arm(): Promise<void> {
    if (this.armed) return;
    if ((await this.ctx.storage.getAlarm()) === null) await this.ctx.storage.setAlarm(Date.now() + SWEEP_EVERY_MS);
    this.armed = true;
  }

  /** Delete what is past its time. Returns how many rows went and whether anything is left that can expire. */
  private sweep(): { removed: number; pending: boolean } {
    const now = Date.now();
    let removed = 0;
    const mail = this.sql.exec("DELETE FROM envelopes WHERE at_ms < ?", now - C.mailboxTtlMs).rowsWritten;
    if (mail > 0) this.recount();
    removed += mail;
    removed += this.sql.exec("DELETE FROM joins WHERE at_ms < ?", now - C.joinTtlMs).rowsWritten;
    removed += this.sql.exec("DELETE FROM windows WHERE start_ms + span_ms <= ?", now).rowsWritten;
    removed += this.sql.exec("DELETE FROM seen WHERE until_ms <= ?", now).rowsWritten;
    const left = this.sql
      .exec(
        `SELECT (SELECT COUNT(*) FROM envelopes) + (SELECT COUNT(*) FROM joins) + (SELECT COUNT(*) FROM windows) +
                (SELECT COUNT(*) FROM seen) AS n`,
      )
      .one();
    return { removed, pending: Number(left.n) > 0 };
  }

  /** The alarm: the 7-day mailbox (and the 1-day join requests) expire here. Returns the rows removed. */
  private async sweepAndSchedule(): Promise<number> {
    this.armed = false;
    const { removed, pending } = this.sweep();
    if (removed > 0) console.log(`relay: expired ${removed} rows`);
    if (pending) await this.arm();
    else await this.ctx.storage.deleteAlarm();
    return removed;
  }

  async alarm(): Promise<void> {
    await this.sweepAndSchedule();
  }

  // ------------------------------------------------------------------------------------ test hooks

  /** `/v1/_test/{team}/…`, only with TEST_HOOKS=1: look inside, drop mail, tighten limits, age the clock. */
  private async testHook(route: string, request: Request, url: URL): Promise<Response> {
    switch (route) {
      case "test-dump": {
        let out = "";
        for (const table of ["team", "state", "envelopes", "joins", "devices"]) {
          for (const row of this.sql.exec(`SELECT * FROM ${table}`).toArray()) out += `${table}|${JSON.stringify(row)}\n`;
        }
        return new Response(out, { headers: { "content-type": "text/plain" } });
      }
      case "test-drop": {
        const seq = queryU64(url, "seq") ?? 0;
        const gone = this.sql.exec("DELETE FROM envelopes WHERE seq = ?", seq).rowsWritten;
        if (gone > 0) this.recount();
        return json({ dropped: gone });
      }
      case "test-limits": {
        this.override = (await request.json()) as Partial<Limits>;
        this.limitsCache = undefined;
        this.sql.exec("INSERT OR REPLACE INTO kv (key, value) VALUES ('limits', ?)", JSON.stringify(this.override));
        return json({ limits: { ...DEFAULT_LIMITS, ...this.override } });
      }
      case "test-age": {
        // Pretend the stored mail is `ms` older, then run the sweeper as the alarm would.
        const ms = queryU64(url, "ms") ?? 0;
        this.sql.exec("UPDATE envelopes SET at_ms = at_ms - ?", ms);
        this.sql.exec("UPDATE joins SET at_ms = at_ms - ?", ms);
        this.sql.exec("UPDATE windows SET start_ms = start_ms - ?", ms);
        this.sql.exec("UPDATE seen SET until_ms = until_ms - ?", ms);
        return json(await this.testState(await this.sweepAndSchedule()));
      }
      case "test-alarm":
        return json(await this.testState());
      default:
        return refuse(404, "not found");
    }
  }

  private async testState(removed?: number): Promise<Record<string, unknown>> {
    const count = (table: string) => Number(this.sql.exec(`SELECT COUNT(*) AS n FROM ${table}`).one().n);
    const stored = this.sql.exec("SELECT stored_rows, stored_bytes FROM team WHERE id = 1").toArray()[0];
    return {
      removed,
      alarm: await this.ctx.storage.getAlarm(),
      envelopes: count("envelopes"),
      joins: count("joins"),
      windows: count("windows"),
      seen: count("seen"),
      devices: count("devices"),
      storedRows: stored ? Number(stored.stored_rows) : 0,
      storedBytes: stored ? Number(stored.stored_bytes) : 0,
    };
  }
}
