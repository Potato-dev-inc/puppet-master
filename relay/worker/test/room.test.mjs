// Live Room on the Worker: relay/worker/src/room.ts, roomproto.ts and the room routes of index.ts. The same
// rules as relay/server/src/rooms.rs and relay/protocol/src/room.rs, checked against two local `wrangler dev`
// servers that scripts/run-tests.mjs starts (and stops):
//   PM_RELAY_TEST_URL    TEST_HOOKS=1, high per-address limits (the owner and viewer flows)
//   PM_RELAY_STRICT_URL  as deployed: default limits (the registration budget, the viewer budget)
import assert from "node:assert/strict";
import { generateKeyPairSync, randomBytes, sign as edSign } from "node:crypto";
import { request as httpRequest } from "node:http";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { authHeader, b64, base32, client, sha256 } from "./proto.mjs";

const HOOKS = process.env.PM_RELAY_TEST_URL;
const STRICT = process.env.PM_RELAY_STRICT_URL;
assert.ok(HOOKS && STRICT, "set PM_RELAY_TEST_URL and PM_RELAY_STRICT_URL (npm run test:worker does)");

const HOUR = 60 * 60 * 1000;
const GROUP_ORDER = (1n << 252n) + 27742317777372353535851937790883648493n;
const worker = join(dirname(fileURLToPath(import.meta.url)), "..");

/** A fresh Ed25519 key (node:crypto), shaped like proto.mjs's keys: `id`, `publicRaw`, `sign(message)`. */
function newKey() {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const publicRaw = Buffer.from(publicKey.export({ format: "jwk" }).x, "base64url");
  return {
    id: publicRaw.toString("base64url"),
    publicRaw,
    sign: (message) => edSign(null, Buffer.from(message), privateKey).toString("base64url"),
  };
}

/** The room id of a room key: 26 lowercase base32 characters of SHA-256("pm-room-v1" || key). */
const roomIdOf = (publicRaw) => base32(sha256(Buffer.from("pm-room-v1"), publicRaw)).slice(0, 26).toLowerCase();
const readHashOf = (token) => b64(sha256(Buffer.from("pm-room-read-v1"), token));
const seatId = () => randomBytes(16).toString("base64url");

/** The bytes a registration is signed over (relay/protocol `RoomRegistration::signing_bytes`). */
const regBytes = (r) => `pm-room-reg-v1\n${r.v}\n${r.room}\n${r.key}\n${r.expires_at_ms}\n${r.max_viewers}\n${r.read_hash}`;

/** A signed registration for `owner`, and the read token it opens the room with. */
function registrationOf(owner, { expires = Date.now() + HOUR, viewers = 10, token = randomBytes(16), room } = {}) {
  const reg = {
    v: 1,
    room: room ?? roomIdOf(owner.publicRaw),
    key: owner.id,
    expires_at_ms: expires,
    max_viewers: viewers,
    read_hash: readHashOf(token),
    sig: "",
  };
  reg.sig = owner.sign(regBytes(reg));
  return { reg, token };
}

/** A frame of the room's shape: a 12-byte nonce and base64url ciphertext. */
function frameOf(room, seq, kind = "delta", ctBytes = 40) {
  return { v: 1, room, seq, kind, nonce: b64(randomBytes(12)), ct: b64(randomBytes(ctBytes)) };
}

const viewer = (token, seat = seatId()) => ({ "x-pm-room-read": b64(token), "x-pm-room-seat": seat });

/** The rooms this file opened on the test relay, with their owners, so the open-room test can stop them. */
const opened = [];

/** Posts a registration; a room made on the test relay is remembered with its owner. */
async function register(api, reg, owner) {
  const res = await api.call("POST", "/v1/rooms", { body: reg });
  if (res.status === 201 && api.base === HOOKS) opened.push({ room: reg.room, owner });
  return res;
}

/** A room registered with a fresh key and token. */
async function room(api, options) {
  const owner = newKey();
  const { reg, token } = registrationOf(owner, options);
  const res = await register(api, reg, owner);
  assert.equal(res.status, 201, res.text);
  return { owner, token, room: reg.room, api };
}

const owned = (r, method, path, body) => r.api.call(method, `/v1/rooms/${r.room}${path}`, { body, device: r.owner });
const viewed = (r, path, token = r.token, seat = seatId()) =>
  r.api.call("GET", `/v1/rooms/${r.room}${path}`, { headers: viewer(token, seat) });
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** A GET with a body (fetch refuses one, so this uses node:http). */
function getWithBody(url, headers, body) {
  const target = new URL(url);
  return new Promise((resolve, reject) => {
    const req = httpRequest(
      {
        hostname: target.hostname,
        port: target.port,
        path: target.pathname + target.search,
        method: "GET",
        headers: { ...headers, "content-length": Buffer.byteLength(body) },
      },
      (res) => {
        let text = "";
        res.on("data", (chunk) => (text += chunk));
        res.on("end", () => resolve({ status: res.statusCode, text }));
      },
    );
    req.on("error", reject);
    req.end(body);
  });
}

test("the room routes: methods, paths and the viewer page", async () => {
  const api = client(HOOKS);
  const id = roomIdOf(newKey().publicRaw);
  assert.equal((await api.call("GET", "/v1/rooms")).status, 405, "registering is a POST");
  assert.equal((await api.call("GET", `/v1/rooms/${id}`)).status, 405, "only DELETE on the room");
  assert.equal((await api.call("POST", `/v1/rooms/${id}/status`)).status, 405);
  assert.equal((await api.call("GET", `/v1/rooms/${id}/nothing`)).status, 404);
  assert.equal((await api.call("GET", "/v1/rooms/BAD/snapshot", { headers: viewer(randomBytes(16)) })).status, 404, "a malformed room id");

  const page = await api.call("GET", "/r/");
  assert.equal(page.status, 200);
  assert.equal(/<form|<input|<textarea/.test(page.text), false, "no inputs on the page");
  assert.equal((await api.call("POST", "/r/")).status, 405);
  assert.equal((await api.call("GET", "/r/nothing.js")).status, 404);
});

test("the viewer page is served with the strict policy and no-store", async () => {
  const origin = new URL(HOOKS).origin;
  for (const [path, type] of [
    ["/r/", "text/html"],
    ["/r/viewer.js", "text/javascript"],
    ["/r/viewer.css", "text/css"],
  ]) {
    const res = await fetch(origin + path);
    assert.equal(res.status, 200, path);
    assert.match(res.headers.get("content-type") ?? "", new RegExp(`^${type}`), path);
    const csp = res.headers.get("content-security-policy") ?? "";
    for (const part of ["default-src 'none'", "script-src 'self'", "style-src 'self'", "connect-src 'self'", "frame-ancestors 'none'", "form-action 'none'"]) {
      assert.ok(csp.includes(part), `${part} in ${csp}`);
    }
    assert.equal(res.headers.get("referrer-policy"), "no-referrer", path);
    assert.equal(res.headers.get("x-content-type-options"), "nosniff", path);
    assert.equal(res.headers.get("cross-origin-opener-policy"), "same-origin", path);
    assert.equal(res.headers.get("cache-control"), "no-store", path);
  }
});

test("a registration is checked as RoomRegistration::verify checks it, in its order", async () => {
  const api = client(HOOKS);
  const owner = newKey();
  const good = registrationOf(owner);
  const refused = async (reg, why) => {
    const res = await api.call("POST", "/v1/rooms", { body: reg });
    assert.equal(res.status, 400, `${why}: ${res.text}`);
    assert.equal(res.json?.error, why);
  };

  await refused("not json", "bad registration");
  await refused({ ...good.reg, sig: undefined }, "bad registration");
  await refused({ ...good.reg, max_viewers: "10" }, "bad registration");
  await refused({ ...good.reg, room: roomIdOf(newKey().publicRaw) }, "room id does not match its key");
  await refused(registrationOf(owner, { expires: Date.now() - 1000 }).reg, "room expiry is in the past");
  await refused(registrationOf(owner, { expires: Date.now() + 8 * HOUR + 60_000 }).reg, "room lives longer than the relay allows");
  await refused(registrationOf(owner, { viewers: 0 }).reg, "bad viewer limit");
  await refused(registrationOf(owner, { viewers: 51 }).reg, "bad viewer limit");
  await refused({ ...good.reg, max_viewers: 11 }, "bad registration signature");
  await refused({ ...good.reg, sig: newKey().sign(regBytes(good.reg)) }, "bad registration signature");

  // The same signature with S + L: it verifies in plain Ed25519, but it is not canonical, and verify_strict refuses it.
  const sig = Buffer.from(good.reg.sig, "base64url");
  let s = 0n;
  for (let i = 63; i >= 32; i -= 1) s = (s << 8n) | BigInt(sig[i]);
  const shifted = Buffer.alloc(64);
  sig.copy(shifted, 0, 0, 32);
  let t = s + GROUP_ORDER;
  for (let i = 32; i < 64; i += 1) {
    shifted[i] = Number(t & 0xffn);
    t >>= 8n;
  }
  await refused({ ...good.reg, sig: shifted.toString("base64url") }, "bad registration signature");

  // The identity point as the room key: a small-order key is never accepted, whatever the signature.
  const identity = Buffer.from("0100000000000000000000000000000000000000000000000000000000000000", "hex");
  const small = { ...good.reg, key: identity.toString("base64url"), room: roomIdOf(identity), sig: Buffer.alloc(64).toString("base64url") };
  await refused(small, "bad registration signature");

  // A registration over 8 KiB is refused before it is parsed.
  assert.equal((await api.call("POST", "/v1/rooms", { body: "x".repeat(9 * 1024) })).status, 413);
});

test("a valid registration is accepted once; a used id is refused", async () => {
  const api = client(HOOKS);
  const owner = newKey();
  const { reg } = registrationOf(owner);
  const first = await register(api, reg, owner);
  assert.equal(first.status, 201, first.text);
  assert.deepEqual(first.json, { room: reg.room });
  assert.equal(reg.room, roomIdOf(owner.publicRaw), "the room id is the Rust room id of the key");
  const again = await register(api, reg, owner);
  assert.equal(again.status, 409);
  assert.equal(again.json?.error, "room already exists");
});

test("the owner's calls are signed by the room key and not replayed; a viewer cannot write", async () => {
  const api = client(HOOKS);
  const r = await room(api);
  const snap = frameOf(r.room, 1, "snap");

  assert.equal((await api.call("PUT", `/v1/rooms/${r.room}/snapshot`, { body: snap })).status, 401, "unsigned");
  assert.equal((await api.call("PUT", `/v1/rooms/${r.room}/snapshot`, { body: snap, device: newKey() })).status, 403, "another key");
  assert.equal((await owned(r, "PUT", "/snapshot", snap)).status, 200);

  // The read token is not a signature.
  const viewerPost = await api.call("POST", `/v1/rooms/${r.room}/frames`, { body: frameOf(r.room, 2), headers: viewer(r.token) });
  assert.equal(viewerPost.status, 401);
  const viewerPut = await api.call("PUT", `/v1/rooms/${r.room}/snapshot`, { body: snap, headers: viewer(r.token) });
  assert.equal(viewerPut.status, 401);

  // A signed request is remembered: the same header again is a replay.
  const header = authHeader(r.owner, "GET", `/v1/rooms/${r.room}/status`, "");
  const first = await api.call("GET", `/v1/rooms/${r.room}/status`, { headers: { "x-pm-auth": header } });
  assert.equal(first.status, 200, first.text);
  assert.deepEqual(Object.keys(first.json).sort(), ["expiresAtMs", "viewers"]);
  const replay = await api.call("GET", `/v1/rooms/${r.room}/status`, { headers: { "x-pm-auth": header } });
  assert.equal(replay.status, 401);
  assert.equal(replay.json?.error, "request replayed");
});

test("a snapshot and deltas come back to a viewer in order, and only with the read token", async () => {
  const api = client(HOOKS);
  const r = await room(api);
  assert.equal((await viewed(r, "/snapshot")).status, 204, "no snapshot yet");
  assert.equal((await owned(r, "PUT", "/snapshot", frameOf(r.room, 1, "snap"))).status, 200);
  assert.equal((await owned(r, "POST", "/frames", frameOf(r.room, 2))).status, 200);
  assert.equal((await owned(r, "POST", "/frames", frameOf(r.room, 3, "state"))).status, 200);

  const snap = await viewed(r, "/snapshot");
  assert.equal(snap.status, 200);
  assert.equal(snap.json.seq, 1);
  assert.equal(snap.json.kind, "snap");
  assert.equal(snap.json.room, r.room);

  const after1 = await viewed(r, "/frames?after=1&wait=0");
  assert.equal(after1.status, 200);
  assert.deepEqual(after1.json.frames.map((f) => f.seq), [2, 3]);
  assert.equal(after1.json.next, 3);
  assert.equal(after1.json.ended, null);
  const after3 = await viewed(r, "/frames?after=3&wait=0");
  assert.deepEqual(after3.json.frames, []);
  assert.equal(after3.json.next, 3);

  const res = await fetch(`${HOOKS}/v1/rooms/${r.room}/snapshot`, { headers: viewer(r.token) });
  assert.equal(res.headers.get("cache-control"), "no-store");
  assert.equal(res.headers.get("access-control-allow-origin"), null, "no CORS on room routes");
});

test("a wrong read token is the same 404 as an unknown room; a bad seat is 400", async () => {
  const api = client(HOOKS);
  const r = await room(api);
  assert.equal((await owned(r, "PUT", "/snapshot", frameOf(r.room, 1, "snap"))).status, 200);
  assert.equal((await viewed(r, "/snapshot", randomBytes(16))).status, 404, "wrong token");
  assert.equal((await viewed(r, "/snapshot", randomBytes(15))).status, 404, "a token of the wrong size");
  assert.equal((await api.call("GET", `/v1/rooms/${r.room}/snapshot`)).status, 404, "no token");
  const ghost = roomIdOf(newKey().publicRaw);
  const unknown = await viewed({ ...r, room: ghost }, "/snapshot");
  assert.equal(unknown.status, 404);
  assert.equal(unknown.text, (await viewed(r, "/snapshot", randomBytes(16))).text, "the same answer");
  assert.equal((await api.call("GET", `/v1/rooms/${r.room}/snapshot`, { headers: { "x-pm-room-read": b64(r.token), "x-pm-room-seat": "no spaces!" } })).status, 400);
  assert.equal((await viewed(r, "/snapshot")).status, 200, "the right token opens it");
});

test("a viewer cannot send a body, and the owner's body limits refuse before parsing", async () => {
  const api = client(HOOKS);
  const r = await room(api);
  const headers = viewer(r.token);
  assert.equal((await getWithBody(`${HOOKS}/v1/rooms/${r.room}/snapshot`, headers, "x")).status, 413);
  assert.equal((await getWithBody(`${HOOKS}/v1/rooms/${r.room}/frames?after=0`, headers, "x")).status, 413);
  // Over the limits (24 KiB frames, 96 KiB snapshots): 413 before the signature check.
  assert.equal((await api.call("POST", `/v1/rooms/${r.room}/frames`, { body: "x".repeat(25 * 1024) })).status, 413);
  assert.equal((await api.call("PUT", `/v1/rooms/${r.room}/snapshot`, { body: "x".repeat(97 * 1024) })).status, 413);
  assert.equal((await owned(r, "POST", "/frames", frameOf(r.room, 1, "delta", 25 * 1024))).status, 413, "a frame over 24 KiB");
});

test("frames must rise, match their route and their shape", async () => {
  const api = client(HOOKS);
  const r = await room(api);
  assert.equal((await owned(r, "POST", "/frames", frameOf(r.room, 5))).status, 200);
  const refusedWith = async (res, status, why) => {
    assert.equal(res.status, status, res.text);
    assert.equal(res.json?.error, why);
  };
  await refusedWith(await owned(r, "POST", "/frames", frameOf(r.room, 5)), 409, "frame sequence number does not rise");
  await refusedWith(await owned(r, "POST", "/frames", frameOf(r.room, 4)), 409, "frame sequence number does not rise");
  await refusedWith(await owned(r, "PUT", "/snapshot", frameOf(r.room, 5, "snap")), 409, "frame sequence number does not rise");
  await refusedWith(await owned(r, "PUT", "/snapshot", frameOf(r.room, 6, "delta")), 400, "a snapshot must be kind snap");
  await refusedWith(await owned(r, "POST", "/frames", frameOf(r.room, 6, "snap")), 400, "frames are kind delta or state");
  await refusedWith(await owned(r, "POST", "/frames", { ...frameOf(r.room, 6), seq: 0 }), 400, "bad sequence number");
  await refusedWith(await owned(r, "POST", "/frames", { ...frameOf(r.room, 6), nonce: b64(randomBytes(11)) }), 400, "bad nonce");
  await refusedWith(await owned(r, "POST", "/frames", { ...frameOf(r.room, 6), ct: "not base64!" }), 400, "bad ciphertext");
  await refusedWith(await owned(r, "POST", "/frames", frameOf(roomIdOf(newKey().publicRaw), 6)), 400, "the frame is for another room");
  await refusedWith(await owned(r, "POST", "/frames", { ...frameOf(r.room, 6), v: 2 }), 400, "unsupported frame version");
  await refusedWith(await owned(r, "POST", "/frames", "not a frame"), 400, "bad frame");
  assert.equal((await owned(r, "POST", "/frames", frameOf(r.room, 6))).status, 200, "the next one still goes");
});

test("the owner's frames are limited to a burst of ten, then two a second", async () => {
  const api = client(HOOKS);
  const r = await room(api);
  const started = Date.now();
  let accepted = 0;
  let refused = 0;
  for (let seq = 1; seq <= 30; seq += 1) {
    const res = await owned(r, "POST", "/frames", frameOf(r.room, seq));
    if (res.status === 200) {
      accepted += 1;
    } else {
      assert.equal(res.status, 429, res.text);
      assert.equal(res.json?.error, "slow down");
      refused += 1;
    }
  }
  const refill = Math.floor(((Date.now() - started) / 1000) * 2);
  assert.ok(accepted >= 10, `the first ten go through: ${accepted}`);
  assert.ok(accepted <= 10 + refill + 1, `then two a second: ${accepted}`);
  assert.ok(refused > 0, "at least one call is limited");
});

test("a viewer's long poll wakes when a frame arrives", async () => {
  const api = client(HOOKS);
  const r = await room(api);
  assert.equal((await owned(r, "PUT", "/snapshot", frameOf(r.room, 1, "snap"))).status, 200);
  const started = Date.now();
  const poll = viewed(r, "/frames?after=1&wait=5");
  await sleep(300);
  assert.equal((await owned(r, "POST", "/frames", frameOf(r.room, 2))).status, 200);
  const res = await poll;
  assert.equal(res.status, 200);
  assert.equal(res.json.frames.length, 1);
  assert.ok(Date.now() - started < 4000, "woke at once, not at the 5 s wait");
});

test("a stopped room is wiped and answers 410 stopped; its id is refused for now", async () => {
  const api = client(HOOKS);
  const r = await room(api);
  assert.equal((await owned(r, "PUT", "/snapshot", frameOf(r.room, 1, "snap"))).status, 200);
  assert.equal((await owned(r, "POST", "/frames", frameOf(r.room, 2))).status, 200);
  assert.equal((await owned(r, "DELETE", "")).status, 204);

  const gone = await viewed(r, "/snapshot");
  assert.equal(gone.status, 410);
  assert.equal(gone.text, '{"ended":"stopped"}');
  assert.equal((await viewed(r, "/frames?after=0&wait=0")).text, '{"ended":"stopped"}');
  assert.equal((await owned(r, "DELETE", "")).status, 410, "stopping twice");
  assert.equal((await owned(r, "GET", "/status")).status, 410);
  assert.equal((await owned(r, "POST", "/frames", frameOf(r.room, 3))).status, 410);
  assert.equal((await viewed(r, "/snapshot", randomBytes(16))).status, 404, "a wrong token still gets 404");

  const { reg } = registrationOf(r.owner, { token: r.token });
  const again = await register(api, reg, r.owner);
  assert.equal(again.status, 409, "a stopped room's id is refused while its tombstone lasts");
});

test("a room ends at its time: 410 expired for viewers and owners", async () => {
  const api = client(HOOKS);
  const r = await room(api, { expires: Date.now() + 3000 });
  assert.equal((await owned(r, "PUT", "/snapshot", frameOf(r.room, 1, "snap"))).status, 200);
  await sleep(3500);
  const res = await viewed(r, "/snapshot");
  assert.equal(res.status, 410);
  assert.equal(res.text, '{"ended":"expired"}');
  assert.equal((await owned(r, "POST", "/frames", frameOf(r.room, 2))).status, 410);
});

test("the viewer limit: one seat per viewer, the room's limit, and four seats per address", async () => {
  const api = client(HOOKS);
  const one = await room(api, { viewers: 1 });
  assert.equal((await owned(one, "PUT", "/snapshot", frameOf(one.room, 1, "snap"))).status, 200);
  const first = seatId();
  assert.equal((await viewed(one, "/snapshot", one.token, first)).status, 200, "the first seat");
  const full = await viewed(one, "/snapshot", one.token, seatId());
  assert.equal(full.status, 429);
  assert.equal(full.text, '{"full":true}');
  assert.equal((await viewed(one, "/snapshot", one.token, first)).status, 200, "the seat already in the room keeps its place");

  const many = await room(api, { viewers: 10 });
  assert.equal((await owned(many, "PUT", "/snapshot", frameOf(many.room, 1, "snap"))).status, 200);
  for (let n = 0; n < 4; n += 1) {
    assert.equal((await viewed(many, "/snapshot", many.token, seatId())).status, 200, `seat ${n + 1} from one address`);
  }
  assert.equal((await viewed(many, "/snapshot", many.token, seatId())).status, 429, "a fifth from the same address");
});

test("the status counts the active viewer seats", async () => {
  const api = client(HOOKS);
  const r = await room(api);
  assert.equal((await owned(r, "PUT", "/snapshot", frameOf(r.room, 1, "snap"))).status, 200);
  assert.equal((await viewed(r, "/snapshot", r.token, seatId())).status, 200);
  const status = await owned(r, "GET", "/status");
  assert.equal(status.status, 200);
  assert.equal(status.json.viewers, 1, "the viewer seat is active");
});

test("registrations and viewer calls are limited per address (strict defaults)", async () => {
  const api = client(STRICT);
  const first = await room(api, { viewers: 10 });
  assert.equal((await owned(first, "PUT", "/snapshot", frameOf(first.room, 1, "snap"))).status, 200);
  const seat = seatId();
  for (let n = 0; n < 120; n += 1) {
    assert.equal((await viewed(first, "/snapshot", first.token, seat)).status, 200, `view ${n + 1}`);
  }
  const limited = await viewed(first, "/snapshot", first.token, seat);
  assert.equal(limited.status, 429, "the 121st view in a minute");
  assert.equal(limited.json?.error, "slow down");

  for (let n = 2; n <= 10; n += 1) {
    const owner = newKey();
    const { reg } = registrationOf(owner);
    assert.equal((await api.call("POST", "/v1/rooms", { body: reg })).status, 201, `registration ${n}`);
  }
  const owner = newKey();
  const refused = await api.call("POST", "/v1/rooms", { body: registrationOf(owner).reg });
  assert.equal(refused.status, 429, "the eleventh registration from one address in an hour");
  assert.equal(refused.json?.error, "slow down");
});

test("the relay's open rooms are capped at 100; stopping them frees the cap", async () => {
  const api = client(HOOKS);
  let busy;
  for (let n = 0; n < 130 && busy === undefined; n += 1) {
    const owner = newKey();
    const { reg } = registrationOf(owner);
    const res = await register(api, reg, owner);
    if (res.status !== 201) busy = res;
  }
  assert.ok(busy, "the cap refuses a room");
  assert.equal(busy.status, 503);
  assert.equal(busy.json?.error, "the relay is busy, try again shortly");
  // Stop every room this file opened here: the cap has room again for whatever runs on this relay next.
  for (const { room: id, owner } of opened.splice(0)) {
    const res = await api.call("DELETE", `/v1/rooms/${id}`, { device: owner });
    assert.ok(res.status === 204 || res.status === 410, `stop ${id}: ${res.status}`);
  }
  const owner = newKey();
  const { reg } = registrationOf(owner);
  assert.equal((await register(api, reg, owner)).status, 201, "a room fits again");
  await api.call("DELETE", `/v1/rooms/${reg.room}`, { device: owner });
});

test("the room code logs nothing: no console call in the room objects, their checks or the routes", () => {
  for (const file of ["room.ts", "roomproto.ts", "index.ts"]) {
    const text = readFileSync(join(worker, "src", file), "utf8");
    assert.equal(/console\./.test(text), false, `${file} has a console call`);
  }
});
