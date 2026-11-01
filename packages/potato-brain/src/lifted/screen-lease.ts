// Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7, file packages/core/src/screen-lease.ts.
// Copyright 2026 Rakazo contributors. Licensed under the Apache License, Version 2.0;
// see packages/potato-sandbox/LICENSE-APACHE-2.0. Changed by Puppet Master: added the expiry helper screenLeaseHeld,
// and parseScreenLeaseId accepts only plain decimal digits as the fence (Rakazo's Number() also accepted "", " 3" and "3e0").
//
// SPEC (the Rust side mirrors these rules; keep the two in step)
//
// 1. Lease id = `<ownerId>:<fence>`. The fence is the text after the LAST colon and must be
//    ASCII digits only, at most Number.MAX_SAFE_INTEGER. The owner may contain colons (boot ids do).
//    An id with no colon, an empty owner, or a bad fence parses as { ownerId: <whole id>, fence: 0 }.
// 2. Take: a request with no id is refused. It is granted when the screen has no holder, when the
//    holder's id is the same string (a renewal), or when the request's fence is STRICTLY greater
//    than the holder's fence. Equal or lower fence is refused: that is a delayed older execution.
//    Owner is not compared on take, so a newer execution of any owner reclaims the screen.
// 3. Release: granted when there is no request id, no holder, or the same id; otherwise only when
//    the request has the same owner as the holder AND a fence >= the holder's fence. So the run that
//    took the screen can release it after a takeover bumped the fence, and a stale finalizer of an
//    older run cannot release a newer run's lease.
// 4. Expiry: a lease holds the screen while now < expiresAtMs and stops holding it at expiresAtMs.
//    An expired lease blocks nobody (the caller treats the screen as free), but its fence is not
//    forgotten: rule 2 still compares against the last fence, so an old execution cannot come back
//    once a newer one has held the screen.
// 5. Times are epoch milliseconds (numbers). Expiry is checked only where the caller asks.

/** The lease id a run holds the screen under. The fence is written after the run's id. */
export function screenLeaseId(runId: string, fence: number): string {
  return `${runId}:${fence}`;
}

const FENCE_DIGITS = /^\d+$/;

export function parseScreenLeaseId(leaseId: string): { ownerId: string; fence: number } {
  const separator = leaseId.lastIndexOf(":");
  if (separator <= 0) return { ownerId: leaseId, fence: 0 };
  const digits = leaseId.slice(separator + 1);
  const fence = FENCE_DIGITS.test(digits) ? Number(digits) : Number.NaN;
  if (!Number.isSafeInteger(fence)) return { ownerId: leaseId, fence: 0 };
  return { ownerId: leaseId.slice(0, separator), fence };
}

/** Rule 2. A take needs a non-empty id that is the holder's own, or has a higher fence. */
export function canTakeScreenLease(
  existing: string | undefined,
  incoming: string | undefined,
): boolean {
  if (!incoming) return false;
  if (!existing || existing === incoming) return true;
  return parseScreenLeaseId(incoming).fence > parseScreenLeaseId(existing).fence;
}

/** Rule 3. A release by the holder's run, or by any run whose fence is not behind the holder's. */
export function canReleaseScreenLease(
  existing: string | undefined,
  incoming: string | undefined,
): boolean {
  if (!incoming || !existing || existing === incoming) return true;
  const current = parseScreenLeaseId(existing);
  const next = parseScreenLeaseId(incoming);
  return next.ownerId === current.ownerId && next.fence >= current.fence;
}

/** Rule 4. True while the lease still holds the screen at `nowMs`. */
export function screenLeaseHeld(expiresAtMs: number | undefined, nowMs: number): boolean {
  return expiresAtMs !== undefined && nowMs < expiresAtMs;
}
