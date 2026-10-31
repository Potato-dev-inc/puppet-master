import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Agent } from "../src/core/loop.ts";
import { StubProvider } from "../src/core/provider.ts";
import { VaultMemory } from "../src/memory/vaultMemory.ts";
import { appendJournal, listMarkdown, readDoc } from "../src/memory/vault.ts";
import { initialMood } from "../src/persona/mood.ts";

const NOW = () => new Date(2026, 9, 28, 15, 0, 0);

test("recall injects a citable pointer and the prefix stays put", async () => {
  const root = mkdtempSync(join(tmpdir(), "fishd-int-"));
  try {
    appendJournal(root, NOW(), "使用者：我最喜歡吃白飯了，尤其是配鹹魚");
    const memory = new VaultMemory(root);
    const agent = new Agent({
      provider: new StubProvider(["嗯嗯。", "再說一次。"]),
      persona: [{ id: "p", volatility: 0, content: "你是藍色大肥魚。" }],
      mood: initialMood(NOW()),
      memory,
      now: NOW,
    });

    const first = await agent.turn("我喜歡吃什麼？");
    const tip = first.prompt.messages.at(-1)?.content ?? "";
    assert.ok(tip.includes("<memory>"), tip);
    assert.ok(tip.includes("白飯"), tip);
    assert.match(tip, /\[\[journal\/\d{4}\/\d{2}\/\d{4}-\d{2}-\d{2}\.md#L\d+\]\]/);
    assert.ok(tip.includes("現在的你"));

    const second = await agent.turn("那再說一次");
    assert.equal(first.prompt.prefixHash, second.prompt.prefixHash);

    const journals = listMarkdown(root);
    assert.equal(journals.length, 1);
    const doc = readDoc(root, journals[0] ?? "");
    assert.ok(doc.includes("我喜歡吃什麼？"), doc);
    assert.ok(doc.includes("　我：嗯嗯。"), doc);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("old turns get compacted into a checkpoint instead of vanishing", async () => {
  const root = mkdtempSync(join(tmpdir(), "fishd-cp-"));
  try {
    const agent = new Agent({
      provider: new StubProvider(["嗯。", "好。", "喔。"]),
      persona: [{ id: "p", volatility: 0, content: "你是魚。" }],
      mood: initialMood(NOW()),
      now: NOW,
      where: "#bot",
      sessionsDir: root,
      compactEvery: 2,
      hotTurns: 2,
    });

    for (let i = 0; i < 3; i++) await agent.turn(`第 ${i} 句`);
    await new Promise((resolve) => setTimeout(resolve, 80));

    const files = listMarkdown(root);
    assert.ok(
      files.some((file) => file.startsWith("sessions/")),
      files.join(", "),
    );
    assert.ok(agent.transcript.length <= 4, `history=${agent.transcript.length}`);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("she keeps a relationship record for each person", () => {
  const root = mkdtempSync(join(tmpdir(), "fishd-rel-"));
  try {
    const memory = new VaultMemory(root);
    memory.log({
      kind: "heard",
      text: "hi",
      at: NOW(),
      where: "#bot",
      who: "pat",
      whoId: "111",
    });

    const before = memory.person("111");
    assert.equal(before?.closeness, 50, "unknown people start neutral");

    const after = memory.setPerson(
      "111",
      { closeness: 82, impression: "喜歡火鍋，最近很累", note: "今天聊了白飯" },
      NOW(),
    );

    assert.equal(after?.closeness, 82);
    assert.equal(after?.impression, "喜歡火鍋，最近很累");
    assert.ok(after?.notes.some((note) => note.includes("白飯")), JSON.stringify(after?.notes));

    // persists across reloads
    const reloaded = new VaultMemory(root).person("111");
    assert.equal(reloaded?.impression, "喜歡火鍋，最近很累");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("she can whisper a thought and update a relationship in one turn", async () => {
  const root = mkdtempSync(join(tmpdir(), "fishd-self-"));
  try {
    const memory = new VaultMemory(root);
    memory.log({
      kind: "heard",
      text: "hi",
      at: NOW(),
      where: "#bot",
      who: "pat",
      whoId: "111",
    });

    let round = 0;
    const provider = {
      name: "scripted",
      async chat() {
        round++;
        if (round === 1) {
          return {
            text: "",
            model: "scripted",
            usage: {
              promptTokens: 10,
              completionTokens: 5,
              cacheHitTokens: 0,
              cacheMissTokens: 10,
            },
            toolCalls: [
              {
                id: "r1",
                name: "relationship",
                arguments: JSON.stringify({
                  person: "pat",
                  closeness: 90,
                  impression: "喜歡火鍋",
                  note: "聊了白飯",
                }),
              },
            ],
          };
        }
        return {
          text: '嗯。\n::state {"whisper":"他好像真的喜歡火鍋"}',
          model: "scripted",
          usage: {
            promptTokens: 10,
            completionTokens: 5,
            cacheHitTokens: 0,
            cacheMissTokens: 10,
          },
        };
      },
    };

    process.env.FISHD_WHISPER_P = "1";
    try {
      const agent = new Agent({
        provider,
        persona: [{ id: "p", volatility: 0, content: "你是魚。" }],
        mood: initialMood(NOW()),
        now: NOW,
        memory,
        where: "#bot",
      });

      const result = await agent.turn("你覺得 pat 怎樣", {
        speaker: { id: "111", name: "pat" },
        group: true,
      });

      assert.equal(result.whisper, "他好像真的喜歡火鍋");
      assert.ok(result.prompt.messages.at(-1)?.content.includes("你對 pat"), "relation line");

      const profile = memory.person("111");
      assert.equal(profile?.closeness, 90);
      assert.equal(profile?.impression, "喜歡火鍋");
    } finally {
      delete process.env.FISHD_WHISPER_P;
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("the tip carries a clock, so she cannot call today 'days ago'", async () => {
  const agent = new Agent({
    provider: new StubProvider(["嗯。"]),
    persona: [{ id: "p", volatility: 0, content: "你是魚。" }],
    mood: initialMood(NOW()),
    now: NOW,
  });

  const result = await agent.turn("現在幾點");
  const tip = result.prompt.messages.at(-1)?.content ?? "";

  assert.ok(tip.includes("現在："), tip);
  assert.ok(tip.includes("2026-10-28"), tip);
  assert.ok(tip.includes("週三"), tip);
  assert.ok(tip.includes("15:00"), tip);
});

test("a group turn carries speaker identity, not just a text prefix", async () => {
  const root = mkdtempSync(join(tmpdir(), "fishd-int-"));
  try {
    const memory = new VaultMemory(root);
    const agent = new Agent({
      provider: new StubProvider(["嗯。", "你好呀。"]),
      persona: [{ id: "p", volatility: 0, content: "你是魚。" }],
      mood: initialMood(NOW()),
      memory,
      now: NOW,
      where: "#一般",
    });

    await agent.turn("我是 pat", { speaker: { id: "a", name: "pat" }, group: true });
    const second = await agent.turn("我是新來的", {
      speaker: { id: "b", name: "輕輕鬆鬆" },
      group: true,
    });

    const tip = second.prompt.messages.at(-1)?.content ?? "";
    assert.ok(tip.includes("<people>"), tip);
    assert.ok(tip.includes("pat"), tip);
    assert.ok(tip.includes("現在：輕輕鬆鬆"), tip);
    assert.ok(tip.includes("[輕輕鬆鬆] 我是新來的"), "speaker tag must be bracketed");

    const doc = readDoc(root, listMarkdown(root).find((p) => p.startsWith("journal/")) ?? "");
    assert.ok(doc.includes("[[people/b|輕輕鬆鬆]]"), doc);
    assert.ok(doc.includes("　我："), "her reply should be an indented line under the group");

    const node = readDoc(root, "nodes/people/b.md");
    assert.ok(node.includes('type: person'));
    assert.ok(node.includes("輕輕鬆鬆"));
    assert.ok(node.includes("第一次出現在 [[journal/"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a message she ignores is still journaled", async () => {
  const root = mkdtempSync(join(tmpdir(), "fishd-int-"));
  try {
    const memory = new VaultMemory(root);
    const agent = new Agent({
      provider: new StubProvider(),
      persona: [{ id: "p", volatility: 0, content: "你是魚。" }],
      mood: initialMood(NOW()),
      memory,
      now: NOW,
      where: "#一般",
    });

    agent.logHeard({ text: "他們在聊別的事", speaker: { id: "c", name: "阿華" } });

    const doc = readDoc(root, listMarkdown(root).find((p) => p.startsWith("journal/")) ?? "");
    assert.ok(doc.includes("[[people/c|阿華]]：他們在聊別的事"), doc);
    assert.ok(!doc.includes("我："), "no reply should have been recorded");
    assert.ok(existsSync(join(root, "nodes", "people", "c.md")));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a reply carries the preceding messages of the burst", async () => {
  const agent = new Agent({
    provider: new StubProvider(["哼，這種事不用你說我也知道啦。"]),
    persona: [{ id: "p", volatility: 0, content: "你是魚。" }],
    mood: initialMood(NOW()),
    now: NOW,
    where: "#bot",
  });

  const result = await agent.turn("我家肥魚聰明得很", {
    speaker: { id: "a", name: "pat" },
    group: true,
    prior: [
      { name: "pat", text: "他根本沒有權限" },
      { name: "pat", text: "他只能閱讀她的日記" },
      { name: "pat", text: "我不是跑 deepseek harness" },
    ],
  });

  const tip = result.prompt.messages.at(-1)?.content ?? "";
  assert.ok(tip.includes("[pat] 他根本沒有權限"), tip);
  assert.ok(tip.includes("[pat] 他只能閱讀她的日記"), tip);
  assert.ok(tip.includes("[pat] 我不是跑 deepseek harness"), tip);
  assert.ok(tip.includes("[pat] 我家肥魚聰明得很"), tip);
});

test("an unrelated turn carries no memory block", async () => {  const root = mkdtempSync(join(tmpdir(), "fishd-int-"));
  try {
    const memory = new VaultMemory(root);
    const agent = new Agent({
      provider: new StubProvider(),
      persona: [{ id: "p", volatility: 0, content: "你是魚。" }],
      mood: initialMood(NOW()),
      memory,
      now: NOW,
    });
    const r = await agent.turn("嗨");
    const tip = r.prompt.messages.at(-1)?.content ?? "";
    assert.equal(tip.includes("<memory>"), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

