// Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7, files packages/adapters/src/tool-loop.test.ts and the trackToolCallStreak cases of packages/core/src/events.test.ts.
// Copyright 2026 Rakazo contributors. Licensed under the Apache License, Version 2.0;
// see packages/potato-sandbox/LICENSE-APACHE-2.0. Changed by Puppet Master: vitest replaced by node:test and node:assert;
// tool names in the cases are generic (lookup, shell, connector_execute).

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  advanceToolCallLoopGuard,
  isLoopGuardStopText,
  loopGuardStopText,
  toolCallStreakLength,
  trackToolCallStreak,
} from "../src/lifted/tool-loop.ts";
import type { ToolCallStreak } from "../src/lifted/tool-loop.ts";

const EMPTY: ToolCallStreak = { key: undefined, count: 0 };

test("allows a parallel batch with distinct action inputs", () => {
  let streak = EMPTY;
  for (let index = 0; index < 20; index += 1) {
    const result = advanceToolCallLoopGuard(streak, "connector_execute", {
      actionId: "records.get",
      input: { recordId: index + 1 },
    });
    streak = result.streak;
    assert.equal(result.stuck, false);
  }
});

test("stops six consecutive calls with identical inputs", () => {
  let streak = EMPTY;
  let stuck = false;
  for (let index = 0; index < 6; index += 1) {
    const result = advanceToolCallLoopGuard(streak, "shell", { command: "npm test" });
    streak = result.streak;
    stuck = result.stuck;
  }
  assert.equal(stuck, true);
  assert.equal(streak.count, 6);
});

test("five identical calls do not stop", () => {
  let streak = EMPTY;
  for (let index = 0; index < 5; index += 1) {
    const result = advanceToolCallLoopGuard(streak, "shell", { command: "npm test" });
    streak = result.streak;
    assert.equal(result.stuck, false);
  }
});

test("stops calls that differ only by a number in a text argument", () => {
  let streak = EMPTY;
  let calls = 0;
  let stuck = false;
  while (!stuck && calls < 50) {
    calls += 1;
    const result = advanceToolCallLoopGuard(streak, "shell", { command: `echo noop-${calls}` });
    streak = result.streak;
    stuck = result.stuck;
  }
  assert.equal(calls, 25);
  assert.equal(toolCallStreakLength(streak), 25);
});

test("reports the run that stopped it, identical or near-identical, in the stop text", () => {
  const stopText = (command: (call: number) => string): string => {
    let streak = EMPTY;
    for (let call = 1; call < 50; call += 1) {
      const result = advanceToolCallLoopGuard(streak, "shell", { command: command(call) });
      streak = result.streak;
      if (result.stuck) return loopGuardStopText("shell", streak);
    }
    throw new Error("never stopped");
  };

  const identical = stopText(() => "npm test");
  const similar = stopText((call) => `echo noop-${call}`);
  assert.ok(identical.includes(" with the same input 6 times in a row"));
  assert.ok(similar.includes(" with nearly the same input 25 times"));
  assert.equal(isLoopGuardStopText(identical), true);
  assert.equal(isLoopGuardStopText(similar), true);
  assert.equal(isLoopGuardStopText("An ordinary reply"), false);
});

test("keeps the identical stop count when a longer similar streak precedes it", () => {
  let streak = EMPTY;
  for (let call = 1; call <= 10; call += 1) {
    streak = advanceToolCallLoopGuard(streak, "shell", { command: `echo noop-${call}` }).streak;
  }
  for (let call = 0; call < 6; call += 1) {
    streak = advanceToolCallLoopGuard(streak, "shell", { command: "echo noop-11" }).streak;
  }
  assert.ok(loopGuardStopText("shell", streak).includes(" with the same input 6 times in a row"));
});

const sequences: Array<[string, (n: number) => unknown]> = [
  ["shell", (n) => ({ command: `sed -n '${n * 100},${n * 100 + 99}p' big.log` })],
  ["browser_act", (n) => ({ actions: [{ kind: "click", ref: `e${n + 2}` }] })],
  ["browser_navigate", (n) => ({ url: `https://fixture.test/list?page=${n}` })],
];
for (const [name, input] of sequences) {
  test(`lets a 12-step ${name} sequence that differs only by numbers finish`, () => {
    let streak = EMPTY;
    for (let step = 1; step <= 12; step += 1) {
      const result = advanceToolCallLoopGuard(streak, name, input(step));
      streak = result.streak;
      assert.equal(result.stuck, false);
    }
  });
}

test("lets a similar run broken by another call start over", () => {
  let streak = EMPTY;
  for (let index = 0; index < 20; index += 1) {
    const command = index % 5 === 4 ? "pwd" : `echo noop-${index}`;
    const result = advanceToolCallLoopGuard(streak, "shell", { command });
    streak = result.streak;
    assert.equal(result.stuck, false);
  }
});

test("trackToolCallStreak starts a streak of 1 for the first call", () => {
  const next = trackToolCallStreak(EMPTY, "lookup", { cursor: "a" });
  assert.equal(next.key, 'lookup:{"cursor":"a"}');
  assert.equal(next.count, 1);
});

test("trackToolCallStreak increments on a repeat and resets on different args or tool", () => {
  const first = trackToolCallStreak(EMPTY, "lookup", { cursor: "a" });
  assert.equal(trackToolCallStreak(first, "lookup", { cursor: "a" }).count, 2);
  assert.equal(trackToolCallStreak(first, "lookup", { cursor: "b" }).count, 1);
  assert.equal(trackToolCallStreak(first, "other", { cursor: "a" }).count, 1);
});

test("trackToolCallStreak counts calls whose text arguments differ only by digits as one similar streak", () => {
  let streak = trackToolCallStreak(EMPTY, "shell", { command: "echo noop-1" });
  streak = trackToolCallStreak(streak, "shell", { command: "echo noop-22" });
  assert.equal(streak.count, 1);
  assert.equal(streak.shapeCount, 2);
  streak = trackToolCallStreak(streak, "shell", { command: "echo other-3" });
  assert.equal(streak.shapeCount, 1);
});

test("trackToolCallStreak does not blank numbers that are not part of a text argument", () => {
  let streak = trackToolCallStreak(EMPTY, "read_file", { offset: 1 });
  streak = trackToolCallStreak(streak, "read_file", { offset: 2 });
  assert.equal(streak.count, 1);
  assert.equal(streak.shapeCount, 1);
});
