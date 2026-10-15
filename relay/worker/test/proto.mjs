// A second, independent implementation of the client side of the relay protocol (node:crypto only),
// to talk to the Worker from the Worker's own tests. The authority is relay/protocol; the end-to-end
// tests in Rust (`relay/server/tests/protocol.rs`, the app's `tests_v1.rs`) use that crate itself.
import { createHash, createPrivateKey, createPublicKey, sign } from "node:crypto";

export const b64 = (bytes) => Buffer.from(bytes).toString("base64url");
export const sha256 = (...parts) => createHash("sha256").update(Buffer.concat(parts.map((p) => Buffer.from(p)))).digest();
export const hex = (bytes) => Buffer.from(bytes).toString("hex");

const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
export function base32(bytes) {
  let out = "";
  let buffer = 0;
  let bits = 0;
  for (const byte of bytes) {
    buffer = ((buffer << 8) | byte) >>> 0;
    bits += 8;
    while (bits >= 5) {
      bits -= 5;
      out += CROCKFORD[(buffer >>> bits) & 0x1f];
    }
    buffer &= (1 << bits) - 1;
  }
  if (bits > 0) out += CROCKFORD[(buffer << (5 - bits)) & 0x1f];
  return out;
}

/** An Ed25519 key from a seed byte (like the Rust tests' `key(seed)`). */
export function key(seed) {
  const secret = Buffer.alloc(32, seed);
  const privateKey = createPrivateKey({
    key: Buffer.concat([Buffer.from("302e020100300506032b657004220420", "hex"), secret]),
    format: "der",
    type: "pkcs8",
  });
  const publicRaw = Buffer.from(createPublicKey(privateKey).export({ format: "jwk" }).x, "base64url");
  return { privateKey, publicRaw, id: b64(publicRaw), sign: (message) => b64(sign(null, Buffer.from(message), privateKey)) };
}

export const teamId = (rootRaw, salt) => base32(sha256("pm-team-id-v1\0", rootRaw, salt).subarray(0, 16));

export function signedAcl(team, version, devices, root) {
  const sig = root.sign(`pm-acl-v1\n${team}\n${version}\n${devices.join(",")}`);
  return { team, version, devices, sig };
}

export function registration(root, device, salt) {
  const team = teamId(root.publicRaw, salt);
  return { team, root: root.id, salt: b64(salt), acl: signedAcl(team, 1, [device.id], root) };
}

let counter = 0;
export function envelope(team, device, seq) {
  counter += 1;
  const env = {
    v: 1,
    id: `01J${String(Date.now() % 1e9).padStart(9, "0")}${String(counter).padStart(14, "0")}`,
    team,
    epoch: 1,
    from: device.id,
    seq,
    nonce: b64(Buffer.alloc(24, seq)),
    ct: b64("ciphertext the relay cannot read"),
    sig: "",
  };
  const header = `pm-envelope-v1\n${env.v}\n${env.id}\n${env.team}\n${env.epoch}\n${env.from}\n${env.seq}\n${env.nonce}`;
  env.sig = device.sign(`${header}\n${env.ct}`);
  return env;
}

export function authHeader(device, method, pathAndQuery, body = "") {
  const ts = Date.now();
  const message = `pm-relay-auth-v1\n${method.toUpperCase()}\n${pathAndQuery}\n${ts}\n${hex(sha256(body))}`;
  return `v1 ${device.id} ${ts} ${device.sign(message)}`;
}

/** A client of one relay base URL (optionally with a path prefix). */
export function client(base) {
  const url = new URL(base);
  const prefix = url.pathname.replace(/\/$/, "");
  const origin = url.origin;
  async function call(method, path, { body, device, headers = {} } = {}) {
    const full = prefix + path;
    const data = body === undefined ? undefined : typeof body === "string" || body instanceof Uint8Array ? body : JSON.stringify(body);
    const h = { ...headers };
    if (device) h["x-pm-auth"] = authHeader(device, method, full, data ?? "");
    const res = await fetch(origin + full, { method, headers: h, body: data });
    const text = await res.text();
    let json;
    try {
      json = JSON.parse(text);
    } catch {
      json = undefined;
    }
    return { status: res.status, text, json };
  }
  return { call, base };
}
