import { test } from "node:test";
import assert from "node:assert/strict";
import { Agent } from "../src/core/loop.ts";
import { StubProvider } from "../src/core/provider.ts";
import { initialMood } from "../src/persona/mood.ts";

test("editing the persona mid-session does not brick the next turn", async () => {
  const now = new Date("2026-10-30T12:00:00Z");
  let version = 0;

  const agent = new Agent({
    provider: new StubProvider(["唔。", "嗯哼。"]),
    persona: [{ id: "p", volatility: 0, content: "v0" }],
    personaLoader: () => [{ id: "p", volatility: 0, content: `v${version}` }],
    mood: initialMood(now),
    now: () => now,
    hotTurns: 4,
  });

  const first = await agent.turn("hi");
  assert.ok(first.segments.length > 0);

  // Someone saves core.md between turns.
  version = 1;
  const second = await agent.turn("hi again");
  assert.ok(second.segments.length > 0);
});
