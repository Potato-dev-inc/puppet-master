// Things only the Worker variant does (the wire protocol itself is proven by the Rust end-to-end
// tests against it: see ../README.md "Tests"). Run with `npm run test:worker`, which starts two
// `wrangler dev` servers and sets:
//   PM_RELAY_TEST_URL    TEST_HOOKS=1, a high per-address limit
//   PM_RELAY_STRICT_URL  as deployed: default limits, no test hooks
import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { test } from "node:test";
import { b64, client, envelope, key, registration, signedAcl } from "./proto.mjs";

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
  // Join requests share the same budget.
  const join = { team: ghost, invite: "I", enc: b64("e"), ct: b64("c"), at_ms: 1 };
  assert.equal((await api.call("POST", `/v1/teams/${ghost}/joins`, { body: join, ...spoof("203.0.113.7") })).status, 429);
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
  await hook(api, team, "limits", "", "POST", { maxEnvelopes: 2 });
  assert.equal((await post(api, team, device, 1)).status, 200);
  assert.equal((await post(api, team, device, 2)).status, 200);
  assert.equal((await post(api, team, device, 3)).status, 429, "full");
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
  assert.ok(state.windows <= 4, `${state.windows} counters (3 invites and the team's day)`);
});

test("only devices on the access list leave a trace in the replay guard", async () => {
  const { api, team } = await newTeam(HOOKS);
  for (let i = 0; i < 5; i += 1) await api.call("GET", `/v1/teams/${team}/envelopes`, { device: key(50 + i) });
  assert.equal((await hook(api, team, "alarm", "", "GET")).json.seen, 0, "five strangers, nothing stored");
  await poll(api, team);
  assert.equal((await hook(api, team, "alarm", "", "GET")).json.seen, 1);
});
