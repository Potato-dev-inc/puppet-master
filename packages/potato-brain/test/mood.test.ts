import { test } from "node:test";
import assert from "node:assert/strict";
import { derive, initialMood, nudge, render, tick } from "../src/persona/mood.ts";
import type { MoodState } from "../src/persona/mood.ts";

const at = (h: number, m = 0) => new Date(2026, 9, 28, h, m, 0, 0);

test("tick drains energy while awake and raises hunger", () => {
  const base = initialMood(at(22), 80, 20);
  const next = tick(base, at(23));
  assert.ok(next.energy < 80, `energy should drop, got ${next.energy}`);
  assert.ok(next.hunger > 20, `hunger should rise, got ${next.hunger}`);
  assert.equal(next.lastTick, at(23).toISOString());
});

test("tick clamps absurd gaps instead of underflowing", () => {
  const base: MoodState = {
    ...initialMood(new Date(2026, 0, 1), 90, 50),
    lastTick: new Date(2026, 0, 1).toISOString(),
  };
  const next = tick(base, new Date(2026, 1, 1));
  assert.ok(next.energy >= 0 && next.energy <= 100);
  assert.ok(next.hunger >= 0 && next.hunger <= 100);
});

test("tick is a no-op for non-positive dt", () => {
  const base = initialMood(at(12), 50, 50);
  const next = tick(base, at(12));
  assert.deepEqual(next, base);
});

test("3am with low energy derives sleepy", () => {
  const s: MoodState = {
    ...initialMood(at(3), 12, 20),
    mood: "genki",
    overrideTurns: 0,
  };
  assert.equal(derive(s, at(3)).mood, "sleepy");
});

test("lunch with high hunger derives hungry", () => {
  const s: MoodState = {
    ...initialMood(at(12, 30), 70, 90),
    mood: "tsundere",
    overrideTurns: 0,
  };
  assert.equal(derive(s, at(12, 30)).mood, "hungry");
});

test("override suppresses derivation", () => {
  const s: MoodState = {
    ...initialMood(at(3), 5, 10),
    mood: "genki",
    overrideTurns: 2,
  };
  assert.equal(derive(s, at(3)).mood, "genki");
});

test("nudge applies once per conversation", () => {
  const s = initialMood(at(15), 60, 40);
  const first = nudge(s, { mood: "tsundere", why: "被逗" }, at(15), {
    nudgedThisSession: false,
  });
  assert.equal(first.accepted, true);
  assert.equal(first.state.mood, "tsundere");
  assert.equal(first.state.overrideTurns, 3);
  assert.ok(first.log?.includes("tsundere"));

  const second = nudge(first.state, { mood: "genki" }, at(15), {
    nudgedThisSession: true,
  });
  assert.equal(second.accepted, false);
  assert.equal(second.reason, "already-nudged");
});

test("nudge is rejected in serious mode", () => {
  const s = initialMood(at(15));
  const r = nudge(s, { mood: "genki" }, at(15), {
    nudgedThisSession: false,
    serious: true,
  });
  assert.equal(r.accepted, false);
  assert.equal(r.reason, "serious-mode");
});

test("nudge is a no-op when nothing changes", () => {
  const s = initialMood(at(15));
  const r = nudge(s, { mood: s.mood, intensity: s.intensity }, at(15), {
    nudgedThisSession: false,
  });
  assert.equal(r.accepted, false);
  assert.equal(r.reason, "unchanged");
});

test("render exposes effects, never raw numbers", () => {
  const s = initialMood(at(15), 74, 35);
  const line = render(s, at(15), { closeness: "friend", gapHours: 50 });
  assert.equal(/\d+\s*%/.test(line), false, `render leaked a percentage: ${line}`);
  assert.equal(/energy|hunger|精力|飽足/.test(line), false, `render leaked a stat name: ${line}`);
  assert.ok(line.includes("現在的你"));
  assert.ok(line.includes("2 天沒見"));
});

test("render drops all cuteness in serious mode", () => {
  const line = render(initialMood(at(15)), at(15), { serious: true });
  assert.ok(line.includes("可愛元素全部收起來"));
  assert.equal(/\d/.test(line), false);
});
