import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Agent } from "../src/core/loop.ts";
import type { ExtraTool } from "../src/core/loop.ts";
import { CodedError, codeForStatus, codeOf } from "../src/core/errors.ts";
import { cleanProject, helperTool } from "../src/core/helper-tool.ts";
import { StubProvider, makeProvider } from "../src/core/provider.ts";
import { VaultMemory } from "../src/memory/vaultMemory.ts";
import { appendJournal } from "../src/memory/vault.ts";
import { initialMood } from "../src/persona/mood.ts";
import type { ChatRequest, ChatResult, Provider, Usage } from "../src/types.ts";

const usage = (): Usage => ({ promptTokens: 1, completionTokens: 1, cacheHitTokens: 0, cacheMissTokens: 1 });
const NOW = () => new Date(2026, 9, 28, 15, 0, 0);

class Scripted implements Provider {
  readonly name = "scripted";
  calls: ChatRequest[] = [];
  private scripts: ChatResult[];
  constructor(scripts: ChatResult[]) {
    this.scripts = scripts;
  }
  async chat(req: ChatRequest): Promise<ChatResult> {
    this.calls.push({ ...req, messages: req.messages.map((m) => ({ ...m })) });
    return this.scripts.shift() ?? { text: "ok", usage: usage(), model: "scripted" };
  }
}

test("no_memory keeps the vault out: no memory tools, no recalled pointers, no profile", async () => {
  const root = mkdtempSync(join(tmpdir(), "potato-lv-"));
  try {
    appendJournal(root, NOW(), "使用者：我最喜歡吃白飯了，尤其是配鹹魚");
    const memory = new VaultMemory(root);
    const make = (provider: Provider) =>
      new Agent({
        provider,
        persona: [{ id: "p", volatility: 0, content: "persona" }],
        mood: initialMood(NOW()),
        memory,
        now: NOW,
      });

    const full = new Scripted([]);
    const a = await make(full).turn("我喜歡吃什麼？", { tools: "full" });
    assert.ok((a.prompt.messages.at(-1)?.content ?? "").includes("<memory>"));
    assert.ok(full.calls[0]?.tools?.some((t) => t.name === "memory_search"));

    const closed = new Scripted([]);
    const b = await make(closed).turn("我喜歡吃什麼？", { tools: "no_memory" });
    const tip = b.prompt.messages.at(-1)?.content ?? "";
    assert.ok(!tip.includes("<memory>"));
    assert.ok(!tip.includes("白飯"));
    const names = (closed.calls[0]?.tools ?? []).map((t) => t.name);
    for (const gone of ["memory_search", "memory_open", "memory_timeline", "memory_diary", "relationship", "persona_note"]) {
      assert.ok(!names.includes(gone), gone);
    }
    assert.ok(names.includes("self"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a tool the model was not offered is not run, even when it names one", async () => {
  const root = mkdtempSync(join(tmpdir(), "potato-lv-"));
  try {
    appendJournal(root, NOW(), "使用者：祕密是香菜");
    const provider = new Scripted([
      {
        text: "",
        usage: usage(),
        model: "scripted",
        toolCalls: [{ id: "c1", name: "memory_search", arguments: JSON.stringify({ query: "香菜" }) }],
      },
      { text: "no idea", usage: usage(), model: "scripted" },
    ]);
    const agent = new Agent({
      provider,
      persona: [{ id: "p", volatility: 0, content: "persona" }],
      mood: initialMood(NOW()),
      memory: new VaultMemory(root),
      now: NOW,
    });
    await agent.turn("what is the secret?", { tools: "no_memory" });
    const tool = provider.calls[1]?.messages.find((m) => m.role === "tool");
    assert.ok(tool?.content.includes("memory_search"));
    assert.ok(!tool?.content.includes("香菜"), "the vault was not searched");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("extra tools are offered, run with the turn number, and stay out of plain agents", async () => {
  const seen: number[] = [];
  const extra: ExtraTool = {
    spec: { name: "ping", description: "x", parameters: { type: "object", properties: {} } },
    run: (_args, ctx) => {
      seen.push(ctx.turn);
      return "pong";
    },
  };
  const provider = new Scripted([
    { text: "", usage: usage(), model: "m1", toolCalls: [{ id: "c", name: "ping", arguments: "{}" }] },
    { text: "done", usage: usage(), model: "m2" },
  ]);
  const agent = new Agent({
    provider,
    persona: [{ id: "p", volatility: 0, content: "persona" }],
    mood: initialMood(NOW()),
    now: NOW,
    extraTools: [extra],
  });
  const r = await agent.turn("hi");
  assert.ok(provider.calls[0]?.tools?.some((t) => t.name === "ping"));
  assert.equal(provider.calls[1]?.messages.find((m) => m.role === "tool")?.content, "pong");
  assert.deepEqual(seen, [1]);
  assert.equal(r.model, "m2");

  const plain = new Scripted([]);
  await new Agent({
    provider: plain,
    persona: [{ id: "p", volatility: 0, content: "persona" }],
    mood: initialMood(NOW()),
    now: NOW,
  }).turn("hi");
  assert.ok(!(plain.calls[0]?.tools ?? []).some((t) => t.name === "ping"));
  assert.ok(!(plain.calls[0]?.tools ?? []).some((t) => t.name === "ask_helper"));
});

test("a failed turn does not use up the one ask_helper of the next turn", async () => {
  const asked: string[] = [];
  const tool = helperTool({ conversationId: "c", newId: () => "id", emit: (r) => asked.push(r.task) });
  const call = (task: string): ChatResult => ({
    text: "",
    usage: usage(),
    model: "m",
    toolCalls: [{ id: "x", name: "ask_helper", arguments: JSON.stringify({ task }) }],
  });
  let boom = true;
  const provider: Provider = {
    name: "flaky",
    chat: async (req) => {
      const last = req.messages.at(-1);
      if (last?.role === "tool") {
        if (boom) {
          boom = false;
          throw new CodedError("PROVIDER_DOWN", "down");
        }
        return { text: "fine", usage: usage(), model: "m" };
      }
      return call(`task ${asked.length + 1}`);
    },
  };
  const agent = new Agent({
    provider,
    persona: [{ id: "p", volatility: 0, content: "persona" }],
    mood: initialMood(NOW()),
    now: NOW,
    extraTools: [tool],
  });
  await assert.rejects(agent.turn("one"));
  await agent.turn("two");
  assert.equal(asked.length, 2, "the second turn could ask again");
});

test("helper project names: a name passes, a path does not", () => {
  assert.equal(cleanProject("demo-a"), "demo-a");
  assert.equal(cleanProject("  my project  "), "my project");
  for (const bad of ["C:\\x", "/etc", "a/b", "..", "a..b", "", "x".repeat(81), 5, null, "a\nb"]) {
    assert.equal(cleanProject(bad), undefined, String(bad));
  }
});

test("status codes map to protocol codes", () => {
  assert.equal(codeForStatus(401), "PROVIDER_AUTH");
  assert.equal(codeForStatus(403), "PROVIDER_AUTH");
  assert.equal(codeForStatus(402), "PROVIDER_LIMIT");
  assert.equal(codeForStatus(429), "PROVIDER_LIMIT");
  assert.equal(codeForStatus(500), "PROVIDER_DOWN");
  assert.equal(codeForStatus(502), "PROVIDER_DOWN");
  assert.equal(codeForStatus(400), "INTERNAL");
  assert.equal(codeOf(new Error("boom with /some/path")), "INTERNAL");
  assert.equal(codeOf(new CodedError("NETWORK", "x")), "NETWORK");
});

test("FISHD_PROVIDER=stub never reads a key or a login file", () => {
  const saved = { p: process.env.FISHD_PROVIDER, a: process.env.OPENCODE_AUTH_FILE };
  process.env.FISHD_PROVIDER = "stub";
  // a path that would throw if anything tried to read it as a login file
  process.env.OPENCODE_AUTH_FILE = join(tmpdir(), "does", "not", "exist", "auth.json");
  try {
    assert.ok(makeProvider() instanceof StubProvider);
  } finally {
    if (saved.p === undefined) delete process.env.FISHD_PROVIDER;
    else process.env.FISHD_PROVIDER = saved.p;
    if (saved.a === undefined) delete process.env.OPENCODE_AUTH_FILE;
    else process.env.OPENCODE_AUTH_FILE = saved.a;
  }
});
