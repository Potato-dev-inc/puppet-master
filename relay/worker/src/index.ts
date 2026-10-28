// The Puppet Master team relay as a Cloudflare Worker + Durable Objects.
//
// Same wire protocol as relay/server (axum + SQLite), so the app's `RelayTransport` works against
// it unchanged: same routes, same signed requests (`x-pm-auth`), same status codes and messages.
//
//   POST /v1/teams                          register a team (root-signed access list)
//   PUT  /v1/teams/{team}/acl               new access list (root-signed)
//   PUT|GET /v1/teams/{team}/state/{slot}   signed `roster` / `keys` slots
//   POST|GET /v1/teams/{team}/envelopes     the mailbox (GET long-polls with ?after=&wait=)
//   GET  /v1/teams/{team}/ws                the mailbox as a WebSocket (hibernating)
//   POST|GET /v1/teams/{team}/joins         sealed join requests
//   POST     /v1/rooms                      register a Live Room (signed by its room key)
//   PUT|DELETE|GET /v1/rooms/{room}/…       the owner's calls (signed) and the viewers' GETs (read token)
//   GET  /r/                                the Live Room viewer page (static, strict policy)
//   GET  /v1/health
//
// This file is the front door: it sorts the request, caps the body, checks the device signature
// (WebAssembly built from relay/protocol), counts team registrations per address, passes the client's
// address to the team's Durable Object (join requests are limited per address and team there), and
// hands the rest to `team.ts`, which owns every team's state. Live Room routes follow the same rules
// (`roomRoute` below) and are decided by `room.ts`.

import { addressKeyOf } from "./addr";
import { type Env, limitsFrom } from "./env";
import { addressKey } from "./gate";
import { readBody, refuse, json } from "./http";
import { scopedKey } from "./room";
import { authenticate, C, checkRegistration, isTeamId } from "./proto";
import { isRoomId, isSeatId, parseRegistration, readHashOf, registrationError, ROOM, unb64 } from "./roomproto";
// The viewer, built by relay/viewer, is bundled as text (wrangler `[[rules]] type = "Text"`).
import viewerCss from "../../viewer/dist/viewer.css";
import viewerHtml from "../../viewer/dist/viewer.html";
// viewer.js is a plain script, not an ES module, so TypeScript cannot type its import; wrangler bundles it as text.
// @ts-expect-error see above
import viewerJs from "../../viewer/dist/viewer.js";

export { IpGate } from "./gate";
export { Room, RoomIndex } from "./room";
export { Team } from "./team";

interface Spec {
  route: string;
  team?: string;
  slot?: string;
  /** Needs a device signature (`x-pm-auth`). */
  auth: boolean;
  /** Counts against the client address's registration budget (hourly, across all teams). */
  ip: boolean;
  /** Largest body, bytes. */
  max: number;
}

const NO_BODY = new Uint8Array(0);
const KIB = 1024;

/** Which route a request is, or `"method"` for a known path with the wrong method. */
function match(method: string, parts: string[], testHooks: boolean): Spec | "method" | undefined {
  if (parts[0] !== "v1") return undefined;
  if (parts.length === 2 && parts[1] === "teams") {
    return method === "POST" ? { route: "register", auth: false, ip: true, max: 8 * KIB } : "method";
  }
  if (parts[1] === "_test" && testHooks && parts.length === 4) {
    return { route: `test-${parts[3]}`, team: parts[2], auth: false, ip: false, max: 4 * KIB };
  }
  if (parts[1] !== "teams" || parts.length < 4) return undefined;
  const team = parts[2];
  switch (parts[3]) {
    case "acl":
      if (parts.length !== 4) return undefined;
      return method === "PUT" ? { route: "acl", team, auth: false, ip: false, max: 64 * KIB } : "method";
    case "state":
      if (parts.length !== 5) return undefined;
      if (method === "PUT") return { route: "state-put", team, slot: parts[4], auth: true, ip: false, max: C.maxStateBytes };
      return method === "GET" ? { route: "state-get", team, slot: parts[4], auth: true, ip: false, max: 0 } : "method";
    case "envelopes":
      if (parts.length !== 4) return undefined;
      if (method === "POST") return { route: "env-post", team, auth: true, ip: false, max: C.maxEnvelopeBytes };
      return method === "GET" ? { route: "env-get", team, auth: true, ip: false, max: 0 } : "method";
    case "ws":
      if (parts.length !== 4) return undefined;
      return method === "GET" ? { route: "ws", team, auth: true, ip: false, max: 0 } : "method";
    case "joins":
      if (parts.length !== 4) return undefined;
      // Join requests are limited per address *and team*, by the team's object (it is given the address).
      if (method === "POST") return { route: "join-post", team, auth: false, ip: false, max: C.maxJoinBytes };
      return method === "GET" ? { route: "join-get", team, auth: true, ip: false, max: 0 } : "method";
    default:
      return undefined;
  }
}

// ------------------------------------------------------------------------------- Live Rooms

const MINUTE_MS = 60 * 1000;
const HOUR_MS = 60 * MINUTE_MS;
/** The one object that counts the relay's open rooms. */
const ROOM_INDEX_NAME = "open";

/** The viewer page's policy (SPEC-n32 §3.9): scripts and styles come from the relay itself only. */
const VIEWER_CSP =
  "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; " +
  "base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

const VIEWER: Record<string, { body: string; type: string }> = {
  "": { body: viewerHtml, type: "text/html; charset=utf-8" },
  "viewer.js": { body: viewerJs, type: "text/javascript; charset=utf-8" },
  "viewer.css": { body: viewerCss, type: "text/css; charset=utf-8" },
};

/** `/r/`, `/r/viewer.js`, `/r/viewer.css`. Undefined for any other path. */
function viewerPage(request: Request, url: URL): Response | undefined {
  if (url.pathname.includes("/v1/")) return undefined;
  const match = /\/r\/(viewer\.js|viewer\.css)?$/.exec(url.pathname);
  if (match === null) return undefined;
  if (request.method !== "GET") return refuse(405, "method not allowed");
  const file = VIEWER[match[1] ?? ""];
  return noStore(
    new Response(file.body, {
      headers: {
        "content-type": file.type,
        "content-security-policy": VIEWER_CSP,
        "referrer-policy": "no-referrer",
        "x-content-type-options": "nosniff",
        "cross-origin-opener-policy": "same-origin",
      },
    }),
  );
}

/** Room routes: which route a request is, or `"method"` for a known path with the wrong method. */
interface RoomSpec {
  route: "register" | "stop" | "status" | "snapshot-put" | "snapshot-get" | "frames-post" | "frames-get";
  /** Needs the owner's signature (`x-pm-auth`); otherwise a viewer's read token and seat. */
  owner: boolean;
  /** Largest body, bytes (0 refuses any body with 413). */
  max: number;
}

function matchRoom(method: string, parts: string[]): RoomSpec | "method" | undefined {
  if (parts.length === 2) {
    return method === "POST" ? { route: "register", owner: false, max: ROOM.maxRegistrationBytes } : "method";
  }
  if (parts.length === 3) return method === "DELETE" ? { route: "stop", owner: true, max: 0 } : "method";
  if (parts.length !== 4) return undefined;
  switch (parts[3]) {
    case "snapshot":
      if (method === "PUT") return { route: "snapshot-put", owner: true, max: ROOM.maxSnapshotBytes };
      return method === "GET" ? { route: "snapshot-get", owner: false, max: 0 } : "method";
    case "frames":
      if (method === "POST") return { route: "frames-post", owner: true, max: ROOM.maxFrameBytes };
      return method === "GET" ? { route: "frames-get", owner: false, max: 0 } : "method";
    case "status":
      return method === "GET" ? { route: "status", owner: true, max: 0 } : "method";
    default:
      return undefined;
  }
}

/** Every room answer is never cached (a stopped room must not be served from a cache). */
function noStore(res: Response): Response {
  const headers = new Headers(res.headers);
  headers.set("cache-control", "no-store");
  return new Response(res.body, { status: res.status, statusText: res.statusText, headers });
}

async function roomRoute(request: Request, env: Env, url: URL, parts: string[]): Promise<Response> {
  const spec = matchRoom(request.method, parts);
  if (spec === undefined) return noStore(refuse(404, "not found"));
  if (spec === "method") return noStore(refuse(405, "method not allowed"));
  // 1. The size first, before anything is parsed (relay/server's per-route body limit).
  const read = await readBody(request, spec.max);
  if (read instanceof Response) return noStore(read);
  return noStore(await roomHandle(request, env, url, parts, spec, read));
}

async function roomHandle(request: Request, env: Env, url: URL, parts: string[], spec: RoomSpec, body: Uint8Array): Promise<Response> {
  const address = addressKeyOf(request.headers.get("cf-connecting-ip"));
  if (spec.route === "register") return createRoom(request, env, address, body);
  const internal: Record<string, string> = { "x-pm-int-route": spec.route };
  if (spec.owner) {
    // 2. The owner's signature (relay/protocol, strict). Whether its key is the room's is the room's check.
    const header = request.headers.get(C.authHeader);
    if (!header) return refuse(401, "signed request required");
    const verdict = authenticate(header, request.method, url.pathname + url.search, body, Date.now());
    if (!verdict.ok) return refuse(verdict.status, verdict.error);
    internal["x-pm-int-device"] = verdict.device;
    internal["x-pm-int-sig"] = verdict.sig;
  } else {
    // 2. A viewer: counted per address, then the read token's shape (16 bytes) and the seat's shape. A bad
    // token is the same 404 as an unknown room.
    const gate = env.GATE.get(env.GATE.idFromName(await scopedKey("room-view", address)));
    if (!(await gate.hit(MINUTE_MS, limitsFrom(env).roomViewsPerMinute))) return refuse(429, "slow down");
    const token = unb64(request.headers.get(ROOM.readHeader) ?? "");
    if (token === undefined || token.length !== 16) return refuse(404, "not found");
    const seat = request.headers.get(ROOM.seatHeader) ?? "";
    if (!isSeatId(seat)) return refuse(400, "bad seat");
    internal["x-pm-int-read"] = await readHashOf(token);
    internal["x-pm-int-seat"] = seat;
    internal["x-pm-int-addr"] = await scopedKey("room-seat", address);
  }
  // 3. The room id (no object is made for a malformed one), then the room's own object decides the rest.
  const id = parts[2];
  if (!isRoomId(id)) return refuse(404, "not found");
  const answer = await toRoom(env, id, request, internal, body);
  if (spec.route === "stop" && answer.status === 204) await releaseSlot(env, id);
  return answer;
}

/** Registers a room: the address budget, the signature and shape, a slot among the open rooms, then the room. */
async function createRoom(request: Request, env: Env, address: string, body: Uint8Array): Promise<Response> {
  const gate = env.GATE.get(env.GATE.idFromName(await scopedKey("room-reg", address)));
  if (!(await gate.hit(HOUR_MS, limitsFrom(env).roomsPerAddressPerHour))) return refuse(429, "slow down");
  const reg = parseRegistration(body);
  if (reg === undefined) return refuse(400, "bad registration");
  const why = await registrationError(reg, Date.now());
  if (why !== undefined) return refuse(400, why);
  const index = env.ROOM_INDEX.get(env.ROOM_INDEX.idFromName(ROOM_INDEX_NAME));
  const slot = await index.reserve(reg.room, reg.expires_at_ms, limitsFrom(env).roomMaxOpen, Date.now());
  if (slot === "busy") return refuse(503, "the relay is busy, try again shortly");
  if (slot === "exists") return refuse(409, "room already exists");
  const made = await toRoom(env, reg.room, request, { "x-pm-int-route": "create" }, body);
  if (made.status !== 201) {
    await releaseSlot(env, reg.room);
    return made;
  }
  return json({ room: reg.room }, 201);
}

/** Passes a checked request to the room's object, with the internal headers the Worker verified. */
function toRoom(env: Env, room: string, request: Request, internal: Record<string, string>, body: Uint8Array): Promise<Response> {
  const headers = new Headers(request.headers);
  for (const name of [...headers.keys()]) if (name.startsWith("x-pm-int-")) headers.delete(name);
  headers.delete("content-length");
  headers.delete("transfer-encoding");
  for (const [name, value] of Object.entries(internal)) headers.set(name, value);
  const hasBody = request.method !== "GET" && request.method !== "HEAD" && body.byteLength > 0;
  const stub = env.ROOM.get(env.ROOM.idFromName(room));
  return stub.fetch(new Request(request.url, { method: request.method, headers, body: hasBody ? body : undefined }));
}

/** Gives a room's slot back among the open rooms (best effort: the slot ends with the room's time anyway). */
async function releaseSlot(env: Env, room: string): Promise<void> {
  try {
    await env.ROOM_INDEX.get(env.ROOM_INDEX.idFromName(ROOM_INDEX_NAME)).release(room);
  } catch {
    // The index prunes the row when the room's time is up.
  }
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    const page = viewerPage(request, url);
    if (page !== undefined) return page;
    // The relay may live under a path prefix (`https://example.com/pm/v1/...`); the app signs the
    // path it used, prefix included, so verify exactly what arrived.
    const at = url.pathname.indexOf("/v1/");
    if (at < 0) {
      return url.pathname === "/" ? new Response("Puppet Master team relay\n", { headers: { "content-type": "text/plain" } }) : refuse(404, "not found");
    }
    const parts = url.pathname.slice(at + 1).split("/");
    if (parts.length === 2 && parts[1] === "health") {
      return request.method === "GET" ? json({ ok: true, protocol: C.protocol }) : refuse(405, "method not allowed");
    }
    if (parts[1] === "rooms") return roomRoute(request, env, url, parts);
    const spec = match(request.method, parts, env.TEST_HOOKS === "1");
    if (spec === undefined) return refuse(404, "not found");
    if (spec === "method") return refuse(405, "method not allowed");

    // 1. Size first, as relay/server does (its body limit answers before any handler).
    let body = NO_BODY;
    if (spec.max > 0) {
      const read = await readBody(request, spec.max);
      if (read instanceof Response) return read;
      body = read;
    }

    // 2. The device's signature: time, body hash and Ed25519, from relay/protocol.
    let device: string | undefined;
    let sig: string | undefined;
    if (spec.auth) {
      const header = request.headers.get(C.authHeader);
      if (!header) return refuse(401, "signed request required");
      const verdict = authenticate(header, request.method, url.pathname + url.search, body, Date.now());
      if (!verdict.ok) return refuse(verdict.status, verdict.error);
      device = verdict.device;
      sig = verdict.sig;
    }

    // 3. The client's address: `CF-Connecting-IP`, which Cloudflare sets and a client cannot forge, with an
    // IPv6 address reduced to its /64 (as relay/server does). Team registrations are counted per address here;
    // join requests are counted by the team's object, which is handed the (hashed) address.
    let address: string | undefined;
    if (spec.ip || spec.route === "join-post") {
      address = await addressKey(addressKeyOf(request.headers.get("cf-connecting-ip")));
    }
    if (spec.ip && address !== undefined) {
      const gate = env.GATE.get(env.GATE.idFromName(address));
      if (!(await gate.hit(3_600_000, limitsFrom(env).unauthenticatedPerIpPerHour))) return refuse(429, "slow down");
    }

    // 4. Which team's object.
    let team = spec.team;
    if (spec.route === "register") {
      const verdict = checkRegistration(body);
      if (!verdict.ok) return refuse(verdict.status, verdict.error);
      team = verdict.team;
    }
    if (team === undefined || !isTeamId(team)) return refuse(404, "unknown team");

    // 5. Hand over, with what was verified. Client-sent `x-pm-int-*` headers never get through.
    const headers = new Headers(request.headers);
    for (const name of [...headers.keys()]) if (name.startsWith("x-pm-int-")) headers.delete(name);
    headers.delete("content-length");
    headers.delete("transfer-encoding");
    headers.set("x-pm-int-route", spec.route);
    headers.set("x-pm-int-team", team);
    if (spec.slot !== undefined) headers.set("x-pm-int-slot", spec.slot);
    if (spec.route === "join-post" && address !== undefined) headers.set("x-pm-int-addr", address);
    if (device !== undefined && sig !== undefined) {
      headers.set("x-pm-int-device", device);
      headers.set("x-pm-int-sig", sig);
    }
    const stub = env.TEAM.get(env.TEAM.idFromName(team));
    const hasBody = request.method !== "GET" && request.method !== "HEAD";
    return stub.fetch(new Request(request.url, { method: request.method, headers, body: hasBody ? body : undefined }));
  },
} satisfies ExportedHandler<Env>;
