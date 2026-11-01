// Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7, file packages/core/src/screen-lease.test.ts.
// Copyright 2026 Rakazo contributors. Licensed under the Apache License, Version 2.0;
// see packages/potato-sandbox/LICENSE-APACHE-2.0. Changed by Puppet Master: vitest replaced by node:test and node:assert;
// fence parsing, the expiry helper and the spec rules (see the SPEC comment in src/lifted/screen-lease.ts) tested as well.

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  canReleaseScreenLease,
  canTakeScreenLease,
  parseScreenLeaseId,
  screenLeaseHeld,
  screenLeaseId,
} from "../src/lifted/screen-lease.ts";

test("puts the fence after the run id, including boot ids that contain a colon", () => {
  assert.equal(screenLeaseId("run-1", 8), "run-1:8");
  assert.equal(screenLeaseId("boot:abc", 3), "boot:abc:3");
  assert.deepEqual(parseScreenLeaseId("boot:abc:3"), { ownerId: "boot:abc", fence: 3 });
  assert.deepEqual(parseScreenLeaseId("run-1"), { ownerId: "run-1", fence: 0 });
});

test("a fence that is not plain digits leaves the whole id as the owner with fence 0", () => {
  for (const bad of ["run:", "run:-1", "run:1.5", "run:1e3", "run: 3", "run:99999999999999999999", ":3"]) {
    assert.deepEqual(parseScreenLeaseId(bad), { ownerId: bad, fence: 0 }, bad);
  }
  assert.deepEqual(parseScreenLeaseId("run:007"), { ownerId: "run", fence: 7 });
});

test("lets a newer fence reclaim the same bot screen", () => {
  assert.equal(canTakeScreenLease("run-1:1", "run-2:2"), true);
  assert.equal(canTakeScreenLease("run-1:1", "run-1:8"), true);
});

test("rejects a delayed request from an older execution", () => {
  assert.equal(canTakeScreenLease("run-2:2", "run-1:1"), false);
  assert.equal(canTakeScreenLease("run-1:8", "run-1:1"), false);
  assert.equal(canTakeScreenLease("run-2:2", undefined), false);
  assert.equal(canTakeScreenLease("run-1:2", "run-2:2"), false);
});

test("an equal fence from another owner does not take the screen", () => {
  assert.equal(canTakeScreenLease("run-1:4", "run-2:4"), false);
});

test("a take with no holder, or a renewal by the holder, is granted", () => {
  assert.equal(canTakeScreenLease(undefined, "run-1:1"), true);
  assert.equal(canTakeScreenLease("run-1:1", "run-1:1"), true);
  assert.equal(canTakeScreenLease(undefined, undefined), false);
  assert.equal(canTakeScreenLease("run-1:1", ""), false);
});

test("lets the same run release after a takeover fence bump", () => {
  assert.equal(canReleaseScreenLease("run-1:1", "run-1:8"), true);
  assert.equal(canReleaseScreenLease("run-1:1", "run-1:1"), true);
});

test("rejects a stale finalizer after a newer run reclaimed the screen", () => {
  assert.equal(canReleaseScreenLease("run-2:2", "run-1:1"), false);
  assert.equal(canReleaseScreenLease("run-1:1", "run-2:5"), false);
});

test("an expired lease stops holding the screen at its expiry instant", () => {
  assert.equal(screenLeaseHeld(1_000, 999), true);
  assert.equal(screenLeaseHeld(1_000, 1_000), false);
  assert.equal(screenLeaseHeld(undefined, 0), false);
});

test("an expired holder's fence is still the floor for a take", () => {
  // The lease expired, so nobody holds the screen, but a stale run must still not come back.
  const expiredHolder = "run-2:5";
  assert.equal(screenLeaseHeld(1_000, 2_000), false);
  assert.equal(canTakeScreenLease(expiredHolder, "run-1:4"), false);
  assert.equal(canTakeScreenLease(expiredHolder, "run-3:6"), true);
});
