import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Agent } from "../src/core/loop.ts";
import { VaultMemory } from "../src/memory/vaultMemory.ts";
import { initialMood } from "../src/persona/mood.ts";
import type { ChatRequest, ChatResult, Provider, Usage } from "../src/types.ts";

const zero = (): Usage => ({
  promptTokens: 10,
  completionTokens: 5,
  cacheHitTokens: 0,
  cacheMissTokens: 10,
});

class ScriptedProvider implements Provider {
  readonly name = "scripted";
  calls: ChatRequest[] = [];
  private scripts: ChatResult[];

  constructor(scripts: ChatResult[]) {
    this.scripts = scripts;
  }

  async chat(req: ChatRequest): Promise<ChatResult> {
    this.calls.push(req);
    return this.scripts.shift() ?? { text: "……", usage: zero(), model: "scripted" };
  }
}

const at = () => new Date(2026, 9, 29, 10, 0, 0);

test("she searches, reads a file, then answers from it", async () => {
  const root = mkdtempSync(join(tmpdir(), "fishd-tools-"));
  try {
    const memory = new VaultMemory(root);
    memory.log({
      kind: "heard",
      text: "我昨天說我最喜歡吃火鍋",
      at: new Date(2026, 9, 28, 20, 0, 0),
      where: "#一般",
      who: "pat",
      whoId: "111",
    });

    const provider = new ScriptedProvider([
      {
        text: "",
        usage: zero(),
        model: "scripted",
        toolCalls: [
          { id: "c1", name: "memory_search", arguments: JSON.stringify({ query: "火鍋" }) },
        ],
      },
      { text: "你昨天說你最喜歡火鍋。", usage: zero(), model: "scripted" },
    ]);

    const agent = new Agent({
      provider,
      persona: [{ id: "p", volatility: 0, content: "你是魚。" }],
      mood: initialMood(at()),
      memory,
      now: at,
    });

    const result = await agent.turn("我昨天說什麼");
    assert.equal(result.segments[0], "你昨天說你最喜歡火鍋。");

    assert.equal(provider.calls.length, 2, "one round of tools, then the answer");
    const toolMessage = provider.calls[1]?.messages.find((m) => m.role === "tool");
    assert.ok(toolMessage, "the second request must carry the tool result");
    assert.ok(toolMessage.content.includes("journal/"), toolMessage.content);

    assert.ok(
      agent.transcript.every((m) => m.role !== "tool"),
      "tool chatter must not enter the conversation history",
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("she can write a note about herself and see it next turn", async () => {
  const dir = mkdtempSync(join(tmpdir(), "fishd-notes-"));
  try {
    const personaDir = join(dir, "persona");
    const provider = new ScriptedProvider([
      {
        text: "",
        usage: zero(),
        model: "scripted",
        toolCalls: [
          {
            id: "n1",
            name: "persona_note",
            arguments: JSON.stringify({ note: "我喜歡被摸頭" }),
          },
        ],
      },
      { text: "記住了。", usage: zero(), model: "scripted" },
      { text: "嗯。", usage: zero(), model: "scripted" },
    ]);

    const agent = new Agent({
      provider,
      persona: [{ id: "p", volatility: 0, content: "你是魚。" }],
      mood: initialMood(at()),
      now: at,
      notesDir: personaDir,
    });

    await agent.turn("你喜歡什麼");

    const noteFile = join(personaDir, "notes.md");
    assert.ok(existsSync(noteFile));
    assert.ok(readFileSync(noteFile, "utf8").includes("我喜歡被摸頭"));

    const next = await agent.turn("再說一次");
    const tip = next.prompt.messages.at(-1)?.content ?? "";
    assert.ok(tip.includes("<self>"), tip);
    assert.ok(tip.includes("我喜歡被摸頭"), tip);

    assert.equal(
      next.prompt.prefixHash.length,
      16,
      "notes must not have disturbed the prefix",
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("tools can be switched off", async () => {
  const provider = new ScriptedProvider([{ text: "嗯。", usage: zero(), model: "scripted" }]);
  const agent = new Agent({
    provider,
    persona: [{ id: "p", volatility: 0, content: "你是魚。" }],
    mood: initialMood(at()),
    now: at,
  });

  process.env.FISHD_TOOLS = "0";
  try {
    await agent.turn("嗨");
  } finally {
    delete process.env.FISHD_TOOLS;
  }

  assert.equal(provider.calls[0]?.tools, undefined);
});
