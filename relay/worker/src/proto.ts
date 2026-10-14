// The protocol checks, from relay/protocol compiled to WebAssembly (see ../wasm and ../README.md).
// Everything that decides "is this signature / shape / binding valid" lives there, not here.

import * as wasm from "./wasm/pm_relay_wasm.js";
import wasmModule from "./wasm/pm_relay_wasm_bg.wasm";

wasm.initSync({ module: wasmModule });

const encoder = new TextEncoder();

export type Refused = { ok: false; status: number; error: string };
export type Verdict<T> = ({ ok: true } & T) | Refused;

function read<T>(text: string): Verdict<T> {
  return JSON.parse(text) as Verdict<T>;
}

export interface Constants {
  protocol: string;
  authHeader: string;
  maxEnvelopeBytes: number;
  maxJoinBytes: number;
  maxStateBytes: number;
  mailboxTtlMs: number;
  joinTtlMs: number;
  authSkewMs: number;
  slots: string[];
}

const constantsAnswer = read<Constants>(wasm.constants());
if (!constantsAnswer.ok) throw new Error("the protocol module did not start");
export const C: Constants = constantsAnswer;

export const isTeamId = (id: string): boolean => wasm.is_team_id(id);

export const authenticate = (header: string, method: string, pathAndQuery: string, body: Uint8Array, nowMs: number) =>
  read<{ device: string; sig: string; ts: number }>(wasm.authenticate(header, method, pathAndQuery, body, nowMs));

export const checkRegistration = (body: Uint8Array) =>
  read<{ team: string; root: string; salt: string; version: number; devices: string[] }>(wasm.check_registration(body));

export const checkAcl = (body: Uint8Array, team: string, root: string) =>
  read<{ version: number; devices: string[] }>(wasm.check_acl(body, team, root));

export const checkBlob = (body: Uint8Array, team: string, slot: string, root: string) =>
  read<{ version: number }>(wasm.check_blob(body, team, slot, root));

export const checkEnvelope = (body: Uint8Array, team: string, device: string) =>
  read<{ envelope: string }>(wasm.check_envelope(body, team, device));

export const checkJoin = (body: Uint8Array, team: string) =>
  read<{ invite: string; join: string }>(wasm.check_join(body, team));

export const utf8 = (text: string): Uint8Array => encoder.encode(text);
