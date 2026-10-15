import type { IpGate } from "./gate";
import type { Team } from "./team";

/** Bindings and optional settings (all the numbers are strings in wrangler vars). */
export interface Env {
  TEAM: DurableObjectNamespace<Team>;
  GATE: DurableObjectNamespace<IpGate>;
  /** Optional limits; the defaults match relay/server. */
  LIMIT_DEVICE_PER_MINUTE?: string;
  LIMIT_DEVICE_PER_DAY?: string;
  LIMIT_TEAM_PER_MINUTE?: string;
  LIMIT_DEVICE_REQUESTS_PER_MINUTE?: string;
  LIMIT_JOINS_PER_INVITE?: string;
  LIMIT_JOINS_PER_TEAM_PER_DAY?: string;
  LIMIT_JOINS_PER_ADDRESS_PER_HOUR?: string;
  LIMIT_UNAUTH_PER_IP_PER_HOUR?: string;
  LIMIT_MAX_WAIT_S?: string;
  LIMIT_TEAM_MAX_ENVELOPES?: string;
  LIMIT_TEAM_MAX_BYTES?: string;
  LIMIT_SOCKETS_PER_DEVICE?: string;
  /** "1" turns on the /v1/_test hooks. Never set it on a real deployment. */
  TEST_HOOKS?: string;
}

/**
 * Limits (SAAS-PLAN §3.5): the same numbers, names and meaning as `Limits` in relay/server
 * (`device_per_minute` is `devicePerMinute` here, and so on). The test hook sets them by these names.
 */
export interface Limits {
  /** Envelopes a device may post per minute / per day. */
  devicePerMinute: number;
  devicePerDay: number;
  /** Envelopes a whole team may post per minute. */
  teamPerMinute: number;
  /** Every signed request of one device (reads included), per minute. */
  deviceRequestsPerMinute: number;
  /** Join requests naming one invite, for a join request's lifetime. */
  joinsPerInvite: number;
  joinsPerTeamPerDay: number;
  /** Join requests per client address per team, per hour. */
  joinsPerAddressPerHour: number;
  /** Team registrations per client address per hour. */
  unauthenticatedPerIpPerHour: number;
  /** Longest long-poll wait, seconds. */
  maxWaitS: number;
  /** What one team may keep in its mailbox at once; more is refused with 507. */
  teamMaxEnvelopes: number;
  teamMaxBytes: number;
  /** Open WebSockets per device. */
  socketsPerDevice: number;
}

export const DEFAULT_LIMITS: Limits = {
  devicePerMinute: 60,
  devicePerDay: 5_000,
  teamPerMinute: 600,
  deviceRequestsPerMinute: 300,
  joinsPerInvite: 5,
  joinsPerTeamPerDay: 200,
  joinsPerAddressPerHour: 10,
  unauthenticatedPerIpPerHour: 30,
  maxWaitS: 25,
  teamMaxEnvelopes: 20_000,
  teamMaxBytes: 128 * 1024 * 1024,
  socketsPerDevice: 4,
};

function num(raw: string | undefined, fallback: number): number {
  const n = raw === undefined ? NaN : Number(raw);
  return Number.isFinite(n) && n >= 0 ? n : fallback;
}

export function limitsFrom(env: Env): Limits {
  return {
    devicePerMinute: num(env.LIMIT_DEVICE_PER_MINUTE, DEFAULT_LIMITS.devicePerMinute),
    devicePerDay: num(env.LIMIT_DEVICE_PER_DAY, DEFAULT_LIMITS.devicePerDay),
    teamPerMinute: num(env.LIMIT_TEAM_PER_MINUTE, DEFAULT_LIMITS.teamPerMinute),
    deviceRequestsPerMinute: num(env.LIMIT_DEVICE_REQUESTS_PER_MINUTE, DEFAULT_LIMITS.deviceRequestsPerMinute),
    joinsPerInvite: num(env.LIMIT_JOINS_PER_INVITE, DEFAULT_LIMITS.joinsPerInvite),
    joinsPerTeamPerDay: num(env.LIMIT_JOINS_PER_TEAM_PER_DAY, DEFAULT_LIMITS.joinsPerTeamPerDay),
    joinsPerAddressPerHour: num(env.LIMIT_JOINS_PER_ADDRESS_PER_HOUR, DEFAULT_LIMITS.joinsPerAddressPerHour),
    unauthenticatedPerIpPerHour: num(env.LIMIT_UNAUTH_PER_IP_PER_HOUR, DEFAULT_LIMITS.unauthenticatedPerIpPerHour),
    maxWaitS: num(env.LIMIT_MAX_WAIT_S, DEFAULT_LIMITS.maxWaitS),
    teamMaxEnvelopes: num(env.LIMIT_TEAM_MAX_ENVELOPES, DEFAULT_LIMITS.teamMaxEnvelopes),
    teamMaxBytes: num(env.LIMIT_TEAM_MAX_BYTES, DEFAULT_LIMITS.teamMaxBytes),
    socketsPerDevice: num(env.LIMIT_SOCKETS_PER_DEVICE, DEFAULT_LIMITS.socketsPerDevice),
  };
}
