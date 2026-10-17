// Things only the Worker variant does (the wire protocol itself is proven by the Rust end-to-end
// tests against it: see ../README.md "Tests"). Run with `npm run test:worker`, which starts two
// `wrangler dev` servers and sets:
//   PM_RELAY_TEST_URL    TEST_HOOKS=1, a high per-address limit
//   PM_RELAY_STRICT_URL  as deployed: default limits, no test hooks
import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { test } from "node:test";
import { authHeader, b64, client, envelope, key, registration, signedAcl } from "./proto.mjs";

const HOOKS = process.env.PM_RELAY_TEST_URL;
const STRICT = process.env.PM_RELAY_STRICT_URL;
assert.ok(HOOKS && STRICT, "set PM_RELAY_TEST_URL and PM_RELAY_STRICT_URL (npm run test:worker does)");

const DAY = 24 * 60 * 60 * 1000;
const root = key(1);
const device = key(2);

async function newTeam(base, listed = [device]) {
  const api = client(base);
  const reg = registration(root, device, randomBytes(8));
  if (listed.length !== 1 || listed[0] !== device) reg.acl = signedAcl(reg.team, 1, listed.map((d) => d.id), root);
  const res = await api.call("POST", "/v1/teams", { body: reg });
  assert.equal(res.status, 201, res.text);
  return { api, team: reg.team };
}

const hook = (api, team, action, query = "", method = "POST", body) => api.call(method, `/v1/_test/${team}/${action}${query}`, { body });
const post = (api, team, dev, seq) => api.call("POST", `/v1/teams/${team}/envelopes`, { body: envelope(team, dev, seq), device: dev });
const poll = async (api, team, after = 0) => (await api.call("GET", `/v1/teams/${team}/envelopes?after=${after}`, { device })).json;

test("the front door: health, routes and methods", async () => {
  const api = client(HOOKS);
  const health = await api.call("GET", "/v1/health");
  assert.deepEqual(health.json, { ok: true, protocol: "pm-relay/1" });
  assert.equal((await api.call("GET", "/")).status, 200);
  assert.equal((await api.call("GET", "/v1/nothing")).status, 404);
  assert.equal((await api.call("GET", "/v1/teams")).status, 405, "registering is a POST");
  assert.equal((await api.call("DELETE", "/v1/health")).status, 405);
});

test("an unknown team is a 404 after the signature checks out, and never a 5xx", async () => {
  const api = client(HOOKS);
  const ghost = "0".repeat(26);
  assert.equal((await api.call("GET", `/v1/teams/${ghost}/envelopes`, { device })).status, 404);
  assert.equal((await api.call("GET", `/v1/teams/${ghost}/envelopes`)).status, 401, "unsigned: refused first");
  assert.equal((await api.call("GET", "/v1/teams/not-a-team-id/envelopes", { device })).status, 404);
});

test("bodies over the limit are refused with 413 before anything else", async () => {
  const { api, team } = await newTeam(HOOKS);
  const big = "x".repeat(25 * 1024);
  const res = await api.call("POST", `/v1/teams/${team}/envelopes`, { body: big, device });
  assert.equal(res.status, 413);
  assert.equal((await api.call("POST", "/v1/teams", { body: "y".repeat(9 * 1024) })).status, 413);
  assert.equal((await api.call("POST", `/v1/teams/${team}/joins`, { body: "z".repeat(5 * 1024) })).status, 413);
});

test("a client cannot set the internal headers the front door gives the team object", async () => {
  const { api, team } = await newTeam(HOOKS);
  const outsider = key(9);
  // Signed by an outsider, but claiming to be the listed device: the verified signature wins.
  const res = await api.call("GET", `/v1/teams/${team}/envelopes`, {
    device: outsider,
    headers: { "x-pm-int-device": device.id, "x-pm-int-sig": "forged", "x-pm-int-route": "env-get" },
  });
  assert.equal(res.status, 403);
  // No signature at all, but the internal headers filled in: still unsigned.
  const bare = await api.call("GET", `/v1/teams/${team}/envelopes`, { headers: { "x-pm-int-device": device.id, "x-pm-int-sig": "s" } });
  assert.equal(bare.status, 401);
});

test("the mailbox expires after 7 days and join requests after 1, by the alarm", async () => {
  const { api, team } = await newTeam(HOOKS);
  const join = { team, invite: "INVITE1", enc: b64("enc"), ct: b64("sealed"), at_ms: Date.now() };
  assert.equal((await api.call("POST", `/v1/teams/${team}/joins`, { body: join })).status, 202);
  assert.equal((await post(api, team, device, 1)).status, 200);

  // The alarm is set as soon as there is something that can expire.
  const armed = (await hook(api, team, "alarm", "", "GET")).json;
  assert.equal(typeof armed.alarm, "number");
  assert.ok(armed.alarm > Date.now(), "scheduled in the future");
  assert.equal(armed.envelopes, 1);
  assert.equal(armed.joins, 1);

  // Just under a day: both are still there.
  let state = (await hook(api, team, "age", `?ms=${DAY - 60_000}`)).json;
  assert.equal(state.envelopes, 1);
  assert.equal(state.joins, 1);
  // Past a day: the join request goes, the envelope stays.
  state = (await hook(api, team, "age", `?ms=${2 * 60_000}`)).json;
  assert.equal(state.joins, 0, "join requests last 1 day");
  assert.equal(state.envelopes, 1);
  assert.equal((await poll(api, team)).items.length, 1);
  const joins = await api.call("GET", `/v1/teams/${team}/joins`, { device });
  assert.deepEqual(joins.json, []);
  // Just under 7 days in total: still there. Past 7 days: gone, and so is the read (not only the row).
  state = (await hook(api, team, "age", `?ms=${6 * DAY - 120_000}`)).json;
  assert.equal(state.envelopes, 1, "6 days 23 hours 58 minutes");
  state = (await hook(api, team, "age", `?ms=${5 * 60_000}`)).json;
  assert.equal(state.envelopes, 0, "mail lasts 7 days");
  assert.ok(state.removed >= 1);
  assert.deepEqual((await poll(api, team)).items, []);
  // The requests above left replay-guard rows behind (11 minutes each); once they pass too, nothing
  // can expire any more and the alarm is not scheduled again until the next message.
  state = (await hook(api, team, "age", `?ms=${60 * 60_000}`)).json;
  assert.equal(state.alarm, null, "nothing left to expire: no more alarms until the next message");
});

test("the relay's cursor never goes back, even after every old envelope has expired", async () => {
  const { api, team } = await newTeam(HOOKS);
  const first = (await post(api, team, device, 1)).json.seq;
  const second = (await post(api, team, device, 2)).json.seq;
  assert.equal(second, first + 1);
  await hook(api, team, "age", `?ms=${8 * DAY}`);
  assert.deepEqual((await poll(api, team)).items, []);
  const third = (await post(api, team, device, 3)).json.seq;
  assert.ok(third > second, `a fresh envelope (${third}) comes after the expired ones (${second}), so a client's cursor still finds it`);
  assert.equal((await poll(api, team, second)).items.length, 1);
});

test("a hit on the per-minute team limit is a 429 that clears with the minute, and counts are kept in storage", async () => {
  const { api, team } = await newTeam(HOOKS);
  await hook(api, team, "limits", "", "POST", { teamPerMinute: 2 });
  assert.equal((await post(api, team, device, 1)).status, 200);
  assert.equal((await post(api, team, device, 2)).status, 200);
  assert.equal((await post(api, team, device, 3)).status, 429);
  await hook(api, team, "age", `?ms=${61_000}`); // the window is over
  assert.equal((await post(api, team, device, 4)).status, 200);
});

test("deployed as shipped, there are no test hooks and unauthenticated calls are limited per address", async () => {
  const api = client(STRICT);
  const ghost = "0".repeat(26);
  assert.equal((await hook(api, ghost, "dump", "", "GET")).status, 404, "TEST_HOOKS is off");
  const spoof = (ip) => ({ headers: { "cf-connecting-ip": ip } });
  // 30 an hour from one address (counted even when the request is wrong), the 31st is slowed down.
  let last = 0;
  for (let i = 0; i < 31; i += 1) last = (await api.call("POST", "/v1/teams", { body: "{}", ...spoof("203.0.113.7") })).status;
  assert.equal(last, 429);
  // Another address is not affected.
  assert.equal((await api.call("POST", "/v1/teams", { body: "{}", ...spoof("203.0.113.8") })).status, 400);
  // Join requests have their own budget (per address and team, inside the team's object), so they are not
  // refused for the registrations above: an unknown team is simply a 404.
  const join = { team: ghost, invite: "I", enc: b64("e"), ct: b64("c"), at_ms: 1 };
  assert.equal((await api.call("POST", `/v1/teams/${ghost}/joins`, { body: join, ...spoof("203.0.113.7") })).status, 404);
});

test("an IPv6 client is counted by its /64, and an IPv4-mapped one as its IPv4 address", async () => {
  const api = client(STRICT);
  const from = (ip) => ({ headers: { "cf-connecting-ip": ip } });
  // Registrations: 30 an hour from one /64, however the low 64 bits change.
  let last = 0;
  for (let i = 0; i < 31; i += 1) last = (await api.call("POST", "/v1/teams", { body: "{}", ...from(`2001:db8:aaaa:1:${i.toString(16)}::1`) })).status;
  assert.equal(last, 429, "one /64 shares one budget");
  assert.equal((await api.call("POST", "/v1/teams", { body: "{}", ...from("2001:db8:aaaa:2::1") })).status, 400, "the next /64 is another client");
  let mapped = 0;
  for (let i = 0; i < 31; i += 1) mapped = (await api.call("POST", "/v1/teams", { body: "{}", ...from(i % 2 ? "::ffff:198.51.100.9" : "198.51.100.9") })).status;
  assert.equal(mapped, 429, "::ffff:a.b.c.d is a.b.c.d");
});

test("join requests are limited per address within a team, by /64, and another team is not affected", async () => {
  const { api, team } = await newTeam(HOOKS);
  await hook(api, team, "limits", "", "POST", { joinsPerAddressPerHour: 2 });
  const join = (n) => ({ team, invite: `INV${n}`, enc: b64("e"), ct: b64("c"), at_ms: Date.now() });
  const send = (n, ip) => api.call("POST", `/v1/teams/${team}/joins`, { body: join(n), headers: { "cf-connecting-ip": ip } });
  assert.equal((await send(1, "2001:db8:1:2::1")).status, 202);
  assert.equal((await send(2, "2001:db8:1:2:ffff::9")).status, 202, "same /64");
  assert.equal((await send(3, "2001:db8:1:2:1234::7")).status, 429, "the third from that /64");
  assert.equal((await send(4, "2001:db8:1:3::1")).status, 202, "another /64");
  assert.equal((await send(5, "203.0.113.5")).status, 202);
  assert.equal((await send(6, "::ffff:203.0.113.5")).status, 202);
  assert.equal((await send(7, "203.0.113.5")).status, 429, "mapped and plain IPv4 are one client");
  // The same address on another team has its own count.
  const other = await newTeam(HOOKS);
  await hook(other.api, other.team, "limits", "", "POST", { joinsPerAddressPerHour: 2 });
  const body = { team: other.team, invite: "X", enc: b64("e"), ct: b64("c"), at_ms: Date.now() };
  const res = await other.api.call("POST", `/v1/teams/${other.team}/joins`, { body, headers: { "cf-connecting-ip": "203.0.113.5" } });
  assert.equal(res.status, 202);
  // No address is stored: the counters are keyed by a hash.
  assert.ok(!(await hook(api, team, "dump", "", "GET")).text.includes("203.0.113"));
});

test("an object from before join points and the storage counters is migrated when it wakes", async () => {
  const newcomer = key(7);
  const { api, team } = await newTeam(HOOKS);
  assert.equal((await post(api, team, device, 1)).status, 200);
  assert.equal((await post(api, team, device, 2)).status, 200);
  const acl = signedAcl(team, 2, [device.id, newcomer.id], root);
  assert.equal((await api.call("PUT", `/v1/teams/${team}/acl`, { body: acl })).status, 204);
  const mine = async (who) => (await api.call("GET", `/v1/teams/${team}/envelopes?after=0`, { device: who })).json;
  assert.deepEqual((await mine(newcomer)).items, [], "added after two envelopes: nothing before");
  // Put the object back as the old code left it, and make it restart.
  await hook(api, team, "legacy");
  const after = await mine(newcomer);
  assert.deepEqual(after.items.map((i) => i.seq), [1, 2], "every listed device starts at 0 after the migration");
  assert.equal(after.head, 2);
  assert.deepEqual((await mine(device)).items.map((i) => i.seq), [1, 2]);
  const state = (await hook(api, team, "alarm", "", "GET")).json;
  assert.equal(state.devices, 2);
  assert.equal(state.storedRows, 2, "the storage counters are filled from the rows");
  assert.ok(state.storedBytes > 0);
});

test("the storage counters follow the mailbox through posts, expiry and drops", async () => {
  const { api, team } = await newTeam(HOOKS);
  for (let seq = 1; seq <= 3; seq += 1) assert.equal((await post(api, team, device, seq)).status, 200);
  let state = (await hook(api, team, "alarm", "", "GET")).json;
  assert.equal(state.storedRows, 3);
  const bytes = state.storedBytes;
  await hook(api, team, "drop", "?seq=2");
  state = (await hook(api, team, "alarm", "", "GET")).json;
  assert.equal(state.storedRows, 2);
  assert.ok(state.storedBytes < bytes);
  state = (await hook(api, team, "age", `?ms=${8 * DAY}`)).json;
  assert.equal(state.storedRows, 0);
  assert.equal(state.storedBytes, 0);
  await hook(api, team, "limits", "", "POST", { teamMaxBytes: 300 });
  const res = await post(api, team, device, 4);
  assert.equal(res.status, 507);
  assert.deepEqual(res.json, { error: "the team's mailbox is full" });
});

test("the relay can live under a path prefix: the app signs the path it used", async () => {
  const prefixed = client(`${HOOKS}/pm`);
  assert.deepEqual((await prefixed.call("GET", "/v1/health")).json, { ok: true, protocol: "pm-relay/1" });
  const reg = registration(root, device, randomBytes(8));
  assert.equal((await prefixed.call("POST", "/v1/teams", { body: reg })).status, 201);
  assert.equal((await prefixed.call("POST", `/v1/teams/${reg.team}/envelopes`, { body: envelope(reg.team, device, 1), device })).status, 200);
  const got = await prefixed.call("GET", `/v1/teams/${reg.team}/envelopes?after=0`, { device });
  assert.equal(got.json.items.length, 1);
});

test("a team's mailbox has a cap, and it frees up as mail expires", async () => {
  const { api, team } = await newTeam(HOOKS);
  await hook(api, team, "limits", "", "POST", { teamMaxEnvelopes: 2 });
  assert.equal((await post(api, team, device, 1)).status, 200);
  assert.equal((await post(api, team, device, 2)).status, 200);
  assert.equal((await post(api, team, device, 3)).status, 507, "full");
  await hook(api, team, "age", `?ms=${8 * DAY}`);
  assert.equal((await post(api, team, device, 4)).status, 200, "room again once the old mail is gone");
});

test("junk invite ids cannot grow the counters: the team's daily budget is spent first", async () => {
  const { api, team } = await newTeam(HOOKS);
  await hook(api, team, "limits", "", "POST", { joinsPerTeamPerDay: 3 });
  for (let i = 0; i < 12; i += 1) {
    const join = { team, invite: `JUNK${i}`, enc: b64("e"), ct: b64("c"), at_ms: Date.now() };
    const res = await api.call("POST", `/v1/teams/${team}/joins`, { body: join });
    assert.equal(res.status, i < 3 ? 202 : 429, `join ${i}`);
  }
  const state = (await hook(api, team, "alarm", "", "GET")).json;
  assert.ok(state.windows <= 5, `${state.windows} counters (3 invites, the team's day and the address)`);
});

test("only devices on the access list leave a trace in the replay guard", async () => {
  const { api, team } = await newTeam(HOOKS);
  for (let i = 0; i < 5; i += 1) await api.call("GET", `/v1/teams/${team}/envelopes`, { device: key(50 + i) });
  assert.equal((await hook(api, team, "alarm", "", "GET")).json.seen, 0, "five strangers, nothing stored");
  await poll(api, team);
  assert.equal((await hook(api, team, "alarm", "", "GET")).json.seen, 1);
});

// ----------------------------------------------------------------------------- what Cloudflare bills

const cost = async (api, team, reset = true) => (await hook(api, team, "cost", reset ? "?reset=1" : "")).json;

test("a message costs a bounded number of storage rows (the free plan allows 100,000 written a day)", async () => {
  const { api, team } = await newTeam(HOOKS);
  await post(api, team, device, 1); // the first one also creates the rate-limit rows
  await cost(api, team);
  for (let i = 2; i <= 6; i += 1) assert.equal((await post(api, team, device, i)).status, 200);
  const five = await cost(api, team);
  // Per post: the replay guard (a row and its expiry index), four counters, the envelope, the team's counters.
  assert.ok(five.written <= 5 * 8, `a post wrote ${five.written / 5} rows, budget 8`);
  assert.ok(five.read <= 5 * 14, `a post read ${five.read / 5} rows, budget 14`);
  // A read (the poll, the socket's first call) costs the replay guard and the request counter.
  await poll(api, team, 3);
  const read = await cost(api, team);
  assert.ok(read.written <= 6, `a read wrote ${read.written} rows, budget 6`);
});

test("the expiry sweep reads and writes what expires, not the whole mailbox", async () => {
  const { api, team } = await newTeam(HOOKS);
  await hook(api, team, "limits", "", "POST", { devicePerMinute: 1000, teamPerMinute: 1000, deviceRequestsPerMinute: 1000 });
  for (let i = 1; i <= 120; i += 1) assert.equal((await post(api, team, device, i)).status, 200);
  // Let the replay guard and the counters of those 120 posts expire first (they are swept too), then ...
  await hook(api, team, "age", `?ms=${30 * 60_000}`);
  // ... six days on: nothing expires. The sweep must not read the 120 envelopes to find that out.
  let state = (await hook(api, team, "age", `?ms=${6 * DAY}`)).json;
  assert.equal(state.envelopes, 120);
  assert.ok(state.sweepCost.read < 40, `a sweep with nothing to expire read ${state.sweepCost.read} rows`);
  assert.equal(typeof state.alarm, "number", "still scheduled: there is mail that can expire");
  // Two days more: all 120 expire, and the storage counters follow.
  state = (await hook(api, team, "age", `?ms=${2 * DAY}`)).json;
  assert.equal(state.envelopes, 0);
  assert.equal(state.storedRows, 0);
  assert.equal(state.storedBytes, 0);
  assert.ok(state.sweepCost.written < 120 * 2 + 40, `expiring 120 envelopes wrote ${state.sweepCost.written} rows`);
});

test("a partial expiry keeps the newer mail and the counters agree with the rows", async () => {
  const { api, team } = await newTeam(HOOKS);
  await post(api, team, device, 1);
  await hook(api, team, "age", `?ms=${5 * DAY}`);
  await post(api, team, device, 2);
  await post(api, team, device, 3);
  const state = (await hook(api, team, "age", `?ms=${3 * DAY}`)).json; // the first is 8 days old, the others 3
  assert.equal(state.envelopes, 2);
  assert.equal(state.storedRows, 2);
  const dump = (await hook(api, team, "dump", "", "GET")).text;
  const bytes = dump.split(String.fromCharCode(10)).filter((l) => l.startsWith("envelopes|")).map((l) => Buffer.byteLength(JSON.parse(l.slice(10)).body)).reduce((a, b) => a + b, 0);
  assert.equal(state.storedBytes, bytes);
  assert.equal((await poll(api, team)).items.length, 2);
});

// ---------------------------------------------------------------------------------------- hibernation

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

test("an idle socket hibernates the team's object, answers a ping, and still gets the next envelope", async (t) => {
  const { api, team } = await newTeam(HOOKS);
  const path = `/v1/teams/${team}/ws?after=0`;
  const ws = new WebSocket(HOOKS.replace("http", "ws") + path, { headers: { "x-pm-auth": authHeader(device, "GET", path) } });
  const frames = [];
  ws.onmessage = (m) => frames.push(String(m.data));
  await new Promise((resolve, reject) => {
    ws.onopen = resolve;
    ws.onerror = () => reject(new Error("the socket did not open"));
  });
  try {
    const waitFor = async (what, ok) => {
      for (let i = 0; i < 100 && !ok(); i += 1) await sleep(50);
      assert.ok(ok(), what);
    };
    ws.send("ping");
    await waitFor("a ping is answered", () => frames.includes("pong"));
    const before = await cost(api, team, false);
    // Idle: the runtime evicts the object from memory (its in-memory counters start again from zero).
    await sleep(14_000);
    ws.send("ping");
    await waitFor("a ping is answered after the idle time", () => frames.filter((f) => f === "pong").length === 2);
    const woke = (await cost(api, team, false)).read < before.read;
    t.diagnostic(woke ? "the object was evicted and woke again" : "the object was NOT evicted in 14 s (hibernation not observed)");
    assert.equal((await post(api, team, device, 1)).status, 200);
    await waitFor("the envelope arrives on the socket that slept through it", () => frames.some((f) => f.startsWith('{"seq":1,')));
    assert.ok(woke, "hibernation: the object should have been evicted while only an idle socket was open");
  } finally {
    ws.close();
  }
});
