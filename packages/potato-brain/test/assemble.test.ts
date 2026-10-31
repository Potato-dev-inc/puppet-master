import { test } from "node:test";
import assert from "node:assert/strict";
import { assemble, assertPrefix } from "../src/core/assemble.ts";
import type { Block } from "../src/types.ts";

const CORE: Block[] = [
  { id: "persona:core", volatility: 0, content: "你是藍色大肥魚。" },
  { id: "persona:rules", volatility: 0, content: "一則訊息最多一個顏文字。" },
];

const tip = (s: string): Block => ({ id: "tip:mood", volatility: 4, content: s });

test("prefix hash survives changes to chain, hot, and tip", () => {
  const a = assemble({
    static: CORE,
    stable: [],
    chain: [],
    hot: [{ role: "user", content: "嗨" }],
    tip: tip("（現在的你：元氣。）"),
  });

  const b = assemble({
    static: CORE,
    stable: [],
    chain: [{ id: "cp-002", volatility: 2, content: "第二段 checkpoint" }],
    hot: [
      { role: "user", content: "嗨" },
      { role: "assistant", content: "嗨呀" },
      { role: "user", content: "在嗎" },
    ],
    tip: tip("（現在的你：想睡。）"),
  });

  assert.equal(a.prefixHash, b.prefixHash);
  assert.notEqual(a.promptHash, b.promptHash);
});

test("prefix hash changes when the static block changes", () => {
  const a = assemble({ static: CORE, stable: [], chain: [], hot: [], tip: null });
  const mutated: Block[] = [
    CORE[0] as Block,
    { id: "persona:rules", volatility: 0, content: "一則訊息最多兩個顏文字。" },
  ];
  const b = assemble({ static: mutated, stable: [], chain: [], hot: [], tip: null });
  assert.notEqual(a.prefixHash, b.prefixHash);
});

test("assertPrefix catches a leaked volatile block", () => {
  const p = assemble({ static: CORE, stable: [], chain: [], hot: [], tip: null });
  assertPrefix(p, p.prefixHash);
  assert.throws(() => assertPrefix(p, "deadbeefdeadbeef"));
});

test("system message is first and holds the static prefix", () => {
  const p = assemble({ static: CORE, stable: [], chain: [], hot: [], tip: null });
  const first = p.messages[0];
  assert.equal(first?.role, "system");
  assert.ok(first?.content.includes("藍色大肥魚"));
});

test("tip is appended to the newest user message, not the system prompt", () => {
  const p = assemble({
    static: CORE,
    stable: [],
    chain: [],
    hot: [{ role: "user", content: "早安" }],
    tip: tip("（現在的你：想睡。）"),
  });
  const last = p.messages[p.messages.length - 1];
  assert.equal(last?.role, "user");
  assert.ok(last?.content.startsWith("早安"));
  assert.ok(last?.content.includes("想睡"));
  assert.equal(p.messages[0]?.content.includes("想睡"), false);
});

test("tip becomes its own user message if the last turn is not user", () => {
  const p = assemble({
    static: CORE,
    stable: [],
    chain: [],
    hot: [{ role: "assistant", content: "晚安" }],
    tip: tip("（現在的你：想睡。）"),
  });
  const last = p.messages[p.messages.length - 1];
  assert.equal(last?.role, "user");
  assert.ok(last?.content.includes("想睡"));
});

test("over-budget static throws instead of silently trimming the prefix", () => {
  assert.throws(
    () =>
      assemble(
        { static: [{ id: "big", volatility: 0, content: "字".repeat(20000) }], stable: [], chain: [], hot: [], tip: null },
        { budget: { static: 100, stable: 100, chain: 100, hot: 100, tip: 100 } },
      ),
    /static prefix over budget/,
  );
});

test("chain is trimmed from the middle, keeping head and tail", () => {
  const chain: Block[] = [];
  for (let i = 0; i < 40; i++) {
    chain.push({ id: `cp-${i}`, volatility: 2, content: `段落 ${i} `.repeat(10) });
  }
  const p = assemble(
    { static: CORE, stable: [], chain, hot: [], tip: null },
    { budget: { static: 1000, stable: 100, chain: 200, hot: 100, tip: 100 } },
  );
  const ids = p.blocks.map((b) => b.id);
  assert.ok(ids.includes("cp-0"));
  assert.ok(ids.includes("cp-39"));
  assert.ok(p.dropped.length > 0);
});
