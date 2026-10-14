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
//   GET  /v1/health
//
// This file is the front door: it sorts the request, caps the body, checks the device signature
// (WebAssembly built from relay/protocol), counts unauthenticated calls per address, and hands the
// rest to the team's Durable Object (`team.ts`), which owns every team's state.

import { type Env, limitsFrom } from "./env";
import { addressKey } from "./gate";
import { readBody, refuse, json } from "./http";
import { authenticate, C, checkRegistration, isTeamId } from "./proto";

export { IpGate } from "./gate";
export { Team } from "./team";

interface Spec {
  route: string;
  team?: string;
  slot?: string;
  /** Needs a device signature (`x-pm-auth`). */
  auth: boolean;
  /** Counts against the client address (unauthenticated writes). */
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
      if (method === "POST") return { route: "join-post", team, auth: false, ip: true, max: C.maxJoinBytes };
      return method === "GET" ? { route: "join-get", team, auth: true, ip: false, max: 0 } : "method";
    default:
      return undefined;
  }
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
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

    // 3. Unauthenticated writes (registrations, join requests): per client address.
    if (spec.ip) {
      const address = request.headers.get("cf-connecting-ip") ?? "unknown";
      const gate = env.GATE.get(env.GATE.idFromName(await addressKey(address)));
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
    if (device !== undefined && sig !== undefined) {
      headers.set("x-pm-int-device", device);
      headers.set("x-pm-int-sig", sig);
    }
    const stub = env.TEAM.get(env.TEAM.idFromName(team));
    const hasBody = request.method !== "GET" && request.method !== "HEAD";
    return stub.fetch(new Request(request.url, { method: request.method, headers, body: hasBody ? body : undefined }));
  },
} satisfies ExportedHandler<Env>;
