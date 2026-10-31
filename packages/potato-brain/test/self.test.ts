import { test } from "node:test";
import assert from "node:assert/strict";
import { applySelf, emptySelf, planDue, renderSelf, tickSelf } from "../src/persona/self.ts";

const at = () => new Date(2026, 9, 30, 21, 0, 0);

test("boredom rises while idle and drops when talked to", () => {
  const base = { ...emptySelf(), boredom: 50 };

  const idle = tickSelf(base, at(), at().getTime() - 30 * 60_000);
  assert.ok(idle.boredom > 50, `${idle.boredom}`);

  const chatty = tickSelf(base, at(), at().getTime() - 60_000);
  assert.ok(chatty.boredom < 50, `${chatty.boredom}`);
});

test("boredom stays inside 0..100", () => {
  const high = tickSelf({ ...emptySelf(), boredom: 99 }, at(), at().getTime() - 600 * 60_000);
  assert.equal(high.boredom, 100);

  const low = tickSelf({ ...emptySelf(), boredom: 3 }, at(), at().getTime() - 1000);
  assert.equal(low.boredom, 0);
});

test("she can set a task, slack off, and plan her next line", () => {
  const { state, log } = applySelf(
    emptySelf(),
    { task: "整理白飯清單", slacking: true, planMinutes: 20, planTopic: "晚餐", planWhy: "想知道他吃了沒" },
    at(),
  );

  assert.equal(state.task?.what, "整理白飯清單");
  assert.equal(state.slacking, true);
  assert.ok(state.plan);
  assert.equal(state.plan?.topic, "晚餐");
  assert.ok(log.length >= 3, log.join(" | "));

  const text = renderSelf(state, at());
  assert.ok(text.includes("摸魚"), text);
  assert.ok(text.includes("白飯清單"), text);
  assert.ok(text.includes("20 分鐘後"), text);
});

test("a plan becomes due exactly at its time", () => {
  const { state } = applySelf(emptySelf(), { planMinutes: 10, planTopic: "睡覺" }, at());
  assert.equal(planDue(state, at()), false);
  assert.equal(planDue(state, new Date(at().getTime() + 10 * 60_000)), true);
});

test("clearing the plan and the task works", () => {
  const first = applySelf(emptySelf(), { task: "寫 code", planMinutes: 5, planTopic: "x" }, at()).state;
  const second = applySelf(first, { task: "", clearPlan: true }, at()).state;

  assert.equal(second.task, null);
  assert.equal(second.plan, null);
});

test("renderSelf is empty when there is nothing to say", () => {
  assert.equal(renderSelf({ ...emptySelf(), boredom: 30 }, at()), "");
});
