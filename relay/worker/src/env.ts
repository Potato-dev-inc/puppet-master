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
  LIMIT_JOINS_PER_INVITE?: string;
  LIMIT_JOINS_PER_TEAM_PER_DAY?: string;
  LIMIT_UNAUTH_PER_IP_PER_HOUR?: string;
  LIMIT_MAX_WAIT_S?: string;
  LIMIT_TEAM_MAX_ENVELOPES?: string;
  /** "1" turns on the /v1/_test hooks. Never set it on a real deployment. */
  TEST_HOOKS?: string;
}

/** Limits (SAAS-PLAN §3.5): the same numbers, names and meaning as `Limits` in relay/server. */
export interface Limits {
  devicePerMinute: number;
  devicePerDay: number;
  teamPerMinute: number;
  joinsPerInvite: number;
  joinsPerTeamPerDay: number;
  unauthenticatedPerIpPerHour: number;
  /** Longest long-poll wait, seconds. */
  maxWaitS: number;
  /** Envelopes one team's mailbox may hold at once (a storage cap; the server has none). */
  maxEnvelopes: number;
}

export const DEFAULT_LIMITS: Limits = {
  devicePerMinute: 60,
  devicePerDay: 5_000,
  teamPerMinute: 600,
  joinsPerInvite: 5,
  joinsPerTeamPerDay: 20,
  unauthenticatedPerIpPerHour: 30,
  maxWaitS: 25,
  maxEnvelopes: 20_000,
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
    joinsPerInvite: num(env.LIMIT_JOINS_PER_INVITE, DEFAULT_LIMITS.joinsPerInvite),
    joinsPerTeamPerDay: num(env.LIMIT_JOINS_PER_TEAM_PER_DAY, DEFAULT_LIMITS.joinsPerTeamPerDay),
    unauthenticatedPerIpPerHour: num(env.LIMIT_UNAUTH_PER_IP_PER_HOUR, DEFAULT_LIMITS.unauthenticatedPerIpPerHour),
    maxWaitS: num(env.LIMIT_MAX_WAIT_S, DEFAULT_LIMITS.maxWaitS),
    maxEnvelopes: num(env.LIMIT_TEAM_MAX_ENVELOPES, DEFAULT_LIMITS.maxEnvelopes),
  };
}
