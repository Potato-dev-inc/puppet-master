import { DurableObject } from "cloudflare:workers";
import { DEFAULT_LIMITS, type Env, type Limits, limitsFrom } from "./env";
import { empty, json, jsonText, queryU64, Refusal, refuse } from "./http";
import { C, checkAcl, checkBlob, checkEnvelope, checkJoin, checkRegistration } from "./proto";

/**
 * One Durable Object per team: the mailbox, the access list, the two signed state slots, the join
 * requests, the replay guard and the rate limits of relay/server, in the object's own SQLite store.
 * Open WebSockets use the hibernation API, so an idle team costs nothing between messages.
 *
 * The Worker (`index.ts`) has already checked the request's signature; it tells this object the
 * route and the verified device in `x-pm-int-*` headers (a Durable Object is not reachable from
 * outside the Worker, and the Worker overwrites those headers). Everything stateful is decided
 * here: team exists, device on the access list, request not replayed, limits, storage.
 */

/** How often the sweeper wakes while anything can still expire. */
const SWEEP_EVERY_MS = 60 * 60 * 1000;
/** Replay guard memory: a signed request is valid for `AUTH_SKEW_MS` either side of now. */
const SEEN_FOR_MS = 2 * C.authSkewMs + 1_000;
const DAY_MS = 24 * 60 * 60 * 1000;
const BATCH = 200;

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

export class Team extends DurableObject<Env> {
  private readonly sql: SqlStorage;
  private metaCache: Meta | undefined;
  private devicesCache: Set<string> | undefined;
  /** Long polls waiting for mail. */
  private readonly waiters = new Set<() => void>();
  private armed = false;
  /** Test hook: limits overridden for this team (only read when TEST_HOOKS is on). */
  private override: Partial<Limits> = {};

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
      CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);
    `);
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

  private devices(): Set<string> {
    if (!this.devicesCache) {
      const row = this.sql.exec("SELECT acl FROM team WHERE id = 1").toArray()[0];
      this.devicesCache = new Set(row ? (JSON.parse(String(row.acl)) as string[]) : []);
    }
    return this.devicesCache;
  }

  private limits(): Limits {
    return { ...limitsFrom(this.env), ...this.override };
  }

  /** Count one more in a window of `spanMs`; false when over `max`. */
  private hit(key: string, spanMs: number, max: number): boolean {
    const now = Date.now();
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

  /** A signed request from a device on the team's access list that was not seen before. Returns the device. */
  private authorize(request: Request): string {
    const device = request.headers.get("x-pm-int-device");
    const sig = request.headers.get("x-pm-int-sig");
    if (!device || !sig) throw new Refusal(401, "signed request required");
    this.requireTeam();
    if (!this.devices().has(device)) throw new Refusal(403, "device not on this team");
    const now = Date.now();
    const seen = this.sql.exec("SELECT until_ms FROM seen WHERE sig = ?", sig).toArray()[0];
    if (seen && Number(seen.until_ms) > now) throw new Refusal(401, "request replayed");
    this.sql.exec(
      "INSERT INTO seen (sig, until_ms) VALUES (?, ?) ON CONFLICT (sig) DO UPDATE SET until_ms = excluded.until_ms",
      sig,
      now + SEEN_FOR_MS,
    );
    return device;
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
        return this.postJoin(new Uint8Array(await request.arrayBuffer()));
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
    this.sql.exec(
      "INSERT INTO team (id, team, root, salt, acl_version, acl, created_ms, next_seq) VALUES (1, ?, ?, ?, ?, ?, ?, 1)",
      verdict.team,
      verdict.root,
      verdict.salt,
      verdict.version,
      JSON.stringify(verdict.devices),
      Date.now(),
    );
    this.metaCache = undefined;
    this.devicesCache = undefined;
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
    this.sql.exec("UPDATE team SET acl = ?, acl_version = ? WHERE id = 1", JSON.stringify(verdict.devices), verdict.version);
    this.metaCache = undefined;
    this.devicesCache = undefined;
    // A device taken off the list loses its open socket too, not only new requests.
    this.pumpAll();
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
    const device = this.authorize(request);
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
    // The counter only grows, so a cursor never meets a number it has already passed, even after
    // every old envelope has expired.
    const next = Number(this.sql.exec("SELECT next_seq FROM team WHERE id = 1").one().next_seq);
    this.sql.exec("UPDATE team SET next_seq = ? WHERE id = 1", next + 1);
    this.sql.exec(
      "INSERT INTO envelopes (seq, sender, at_ms, body) VALUES (?, ?, ?, ?)",
      next,
      device,
      Date.now(),
      verdict.envelope,
    );
    this.pumpAll();
    this.wake();
    return json({ seq: next });
  }

  private rowsAfter(after: number, limit: number): Row[] {
    const oldest = Date.now() - C.mailboxTtlMs;
    return this.sql
      .exec("SELECT seq, at_ms, body FROM envelopes WHERE seq > ? AND at_ms >= ? ORDER BY seq LIMIT ?", after, oldest, limit)
      .toArray()
      .map((r) => ({ seq: Number(r.seq), at_ms: Number(r.at_ms), body: String(r.body) }));
  }

  /** What the relay hands out: the envelope with its place in the mailbox (`Delivered`). */
  private delivered(row: Row): string {
    return `{"seq":${row.seq},"at_ms":${row.at_ms},"envelope":${row.body}}`;
  }

  private async getEnvelopes(request: Request, url: URL): Promise<Response> {
    this.authorize(request);
    const after = queryU64(url, "after") ?? 0;
    const wait = Math.min(queryU64(url, "wait") ?? 0, this.limits().maxWaitS);
    const deadline = Date.now() + wait * 1000;
    for (;;) {
      const rows = this.rowsAfter(after, BATCH);
      if (rows.length > 0 || Date.now() >= deadline) {
        const next = rows.length > 0 ? rows[rows.length - 1].seq : after;
        return jsonText(`{"items":[${rows.map((r) => this.delivered(r)).join(",")}],"next":${next}}`);
      }
      await this.waitForMail(deadline - Date.now());
    }
  }

  /** Resolve when mail arrives for this team or after `ms`. */
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
    const device = this.authorize(request);
    if (request.headers.get("upgrade")?.toLowerCase() !== "websocket") return refuse(426, "this is a WebSocket address");
    const after = queryU64(url, "after") ?? 0;
    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair) as [WebSocket, WebSocket];
    this.ctx.acceptWebSocket(server);
    server.serializeAttachment({ device, cursor: after } satisfies Attachment);
    this.pump(server); // the backlog, then live envelopes as they arrive
    return new Response(null, { status: 101, webSocket: client });
  }

  /** Send a socket everything after its cursor; close it when its device is no longer on the list. */
  private pump(ws: WebSocket): void {
    const attachment = ws.deserializeAttachment() as Attachment | null;
    if (!attachment) return;
    try {
      if (!this.devices().has(attachment.device)) {
        ws.close(1008, "device not on this team");
        return;
      }
      let cursor = attachment.cursor;
      for (;;) {
        const rows = this.rowsAfter(cursor, BATCH);
        for (const row of rows) {
          ws.send(this.delivered(row));
          cursor = row.seq;
        }
        if (rows.length < BATCH) break;
      }
      if (cursor !== attachment.cursor) ws.serializeAttachment({ device: attachment.device, cursor } satisfies Attachment);
    } catch (error) {
      console.debug("relay: socket send failed", error);
    }
  }

  private pumpAll(): void {
    for (const ws of this.ctx.getWebSockets()) this.pump(ws);
  }

  async webSocketMessage(): Promise<void> {
    // Clients only listen; anything they say is ignored.
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

  private postJoin(body: Uint8Array): Response {
    const meta = this.requireTeam();
    const verdict = checkJoin(body, meta.team);
    if (!verdict.ok) return refuse(verdict.status, verdict.error);
    const limits = this.limits();
    if (
      !this.hit(`invite:${verdict.invite}`, C.joinTtlMs, limits.joinsPerInvite) ||
      !this.hit("joins-day", DAY_MS, limits.joinsPerTeamPerDay)
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
    removed += this.sql.exec("DELETE FROM envelopes WHERE at_ms < ?", now - C.mailboxTtlMs).rowsWritten;
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
        for (const table of ["team", "state", "envelopes", "joins"]) {
          for (const row of this.sql.exec(`SELECT * FROM ${table}`).toArray()) out += `${table}|${JSON.stringify(row)}\n`;
        }
        return new Response(out, { headers: { "content-type": "text/plain" } });
      }
      case "test-drop": {
        const seq = queryU64(url, "seq") ?? 0;
        const gone = this.sql.exec("DELETE FROM envelopes WHERE seq = ?", seq).rowsWritten;
        return json({ dropped: gone });
      }
      case "test-limits": {
        this.override = (await request.json()) as Partial<Limits>;
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
    return {
      removed,
      alarm: await this.ctx.storage.getAlarm(),
      envelopes: count("envelopes"),
      joins: count("joins"),
      windows: count("windows"),
      seen: count("seen"),
    };
  }
}
