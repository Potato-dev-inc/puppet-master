// Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7, files packages/adapters/src/tool-loop.ts and packages/core/src/events.ts (trackToolCallStreak, ToolCallStreak only).
// Copyright 2026 Rakazo contributors. Licensed under the Apache License, Version 2.0;
// see packages/potato-sandbox/LICENSE-APACHE-2.0. Changed by Puppet Master: trackToolCallStreak moved here from events.ts;
// no @rakazo imports; the stop text names the brain's tool, and the thresholds are unchanged (6 identical, 25 similar).

/** `shape` is the call with the digits in its text arguments blanked, so `echo n-1`, `echo n-2` match. */
export type ToolCallStreak = {
  key: string | undefined;
  count: number;
  shape?: string;
  shapeCount?: number;
};

export function trackToolCallStreak(streak: ToolCallStreak, name: string, args: unknown): ToolCallStreak {
  const key = `${name}:${JSON.stringify(args)}`;
  const shape = `${name}:${JSON.stringify(args, (_, value: unknown) => (typeof value === "string" ? value.replace(/\d+/g, "#") : value))}`;
  return {
    key,
    count: key === streak.key ? streak.count + 1 : 1,
    shape,
    shapeCount: shape === streak.shape ? (streak.shapeCount ?? 0) + 1 : 1,
  };
}

const LOOP_GUARD_STOP_PREFIX = "I got stuck calling ";
const LOOP_GUARD_STOP_COUNT_PATTERN = " with (?:nearly )?the same input ";
const LOOP_GUARD_STOP_SUFFIX =
  " times in a row without making progress, so I stopped early. Try rephrasing this, or ask me to try a different approach.";
const LOOP_GUARD_STOP_PATTERN = new RegExp(
  `^${LOOP_GUARD_STOP_PREFIX}.+${LOOP_GUARD_STOP_COUNT_PATTERN}\\d+${LOOP_GUARD_STOP_SUFFIX.replaceAll(".", "\\.")}$`,
);

// Same tool, same arguments, this many times in a row means the agent is stuck, not paginating.
export const MAX_CONSECUTIVE_IDENTICAL_TOOL_CALLS = 6;
// Arguments that differ only by digits in their text (`echo noop-1`, `echo noop-2`) can be a real
// sequence such as reading a file in chunks or paging through a site, so they get more room.
export const MAX_CONSECUTIVE_SIMILAR_TOOL_CALLS = 25;

export function loopGuardStopText(toolName: string, streak: ToolCallStreak): string {
  const identical = streak.count >= MAX_CONSECUTIVE_IDENTICAL_TOOL_CALLS;
  const count = identical ? streak.count : toolCallStreakLength(streak);
  return `${LOOP_GUARD_STOP_PREFIX}${toolName} with ${identical ? "" : "nearly "}the same input ${count}${LOOP_GUARD_STOP_SUFFIX}`;
}

export function isLoopGuardStopText(text: string): boolean {
  return LOOP_GUARD_STOP_PATTERN.test(text);
}

/** The longer of the two runs, for the stop message. */
export function toolCallStreakLength(streak: ToolCallStreak): number {
  return Math.max(streak.count, streak.shapeCount ?? 0);
}

/** Tracks one more call. `stuck` is true at 6 identical calls in a row or 25 similar ones. */
export function advanceToolCallLoopGuard(
  streak: ToolCallStreak,
  name: string,
  args: unknown,
): { streak: ToolCallStreak; stuck: boolean } {
  const next = trackToolCallStreak(streak, name, args);
  return {
    streak: next,
    stuck:
      next.count >= MAX_CONSECUTIVE_IDENTICAL_TOOL_CALLS ||
      (next.shapeCount ?? 0) >= MAX_CONSECUTIVE_SIMILAR_TOOL_CALLS,
  };
}
