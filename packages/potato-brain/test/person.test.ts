import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { VaultMemory } from "../src/memory/vaultMemory.ts";

const NOW = () => new Date("2026-10-30T12:00:00Z");

function seed(root: string): VaultMemory {
  const memory = new VaultMemory(root);
  memory.log({
    kind: "heard",
    text: "hi",
    at: NOW(),
    where: "#bot",
    who: "pat",
    whoId: "111",
  });
  return memory;
}

test("a person node grows habits, open items and inside jokes", () => {
  const root = mkdtempSync(join(tmpdir(), "fishd-person-"));
  try {
    const memory = seed(root);
    memory.setPerson(
      "111",
      { habit: "通常 23:00 後出現", pending: "禮拜五要交報告", joke: "顯卡燒了" },
      NOW(),
    );

    const p = new VaultMemory(root).person("111");
    assert.deepEqual(p?.habits, ["通常 23:00 後出現"]);
    assert.ok(p?.pending[0]?.includes("禮拜五要交報告"), JSON.stringify(p?.pending));
    assert.deepEqual(p?.jokes, ["顯卡燒了"]);

    const file = readFileSync(join(root, "nodes/people/111.md"), "utf8");
    assert.ok(file.includes("## 習慣"), file);
    assert.ok(file.includes("## 進行中"), file);
    assert.ok(file.includes("## 一起笑過"), file);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("resolving one open item leaves the others alone", () => {
  const root = mkdtempSync(join(tmpdir(), "fishd-person-"));
  try {
    const memory = seed(root);
    memory.setPerson("111", { pending: "禮拜五要交報告" }, NOW());
    memory.setPerson("111", { pending: "下週想吃火鍋" }, NOW());
    assert.equal(memory.person("111")?.pending.length, 2);

    memory.setPerson("111", { resolve: "報告" }, NOW());
    const pending = memory.person("111")?.pending ?? [];
    assert.equal(pending.length, 1);
    assert.ok(pending[0]?.includes("火鍋"), JSON.stringify(pending));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("warmth and a grudge survive a reload", () => {
  const root = mkdtempSync(join(tmpdir(), "fishd-person-"));
  try {
    const memory = seed(root);
    memory.setPerson(
      "111",
      { warmth: 88, grudgeMinutes: 120, reason: "說過晚安又跑回來" },
      NOW(),
    );

    const p = new VaultMemory(root).person("111");
    assert.equal(p?.warmth, 88);
    assert.equal(p?.lastReason, "說過晚安又跑回來");
    assert.ok(p?.grudgeUntil.startsWith("2026-10-30T14:00"), p?.grudgeUntil);

    memory.setPerson("111", { grudgeMinutes: 0 }, NOW());
    assert.equal(memory.person("111")?.grudgeUntil, "");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a hand-written legacy node still parses", () => {
  const root = mkdtempSync(join(tmpdir(), "fishd-person-"));
  try {
    const dir = join(root, "nodes", "people");
    mkdirSync(dir, { recursive: true });
    writeFileSync(
      join(dir, "222.md"),
      [
        "---",
        'id: "222"',
        "type: person",
        'name: "阿明"',
        "---",
        "",
        "# 阿明",
        "",
        "第一次出現在 [[journal/2026/10/2026-10-30.md#L1]]。",
        "",
        "- 2026-10-01 喜歡喝無糖綠",
        "",
      ].join("\n"),
      "utf8",
    );

    const p = new VaultMemory(root).person("222");
    assert.equal(p?.name, "阿明");
    assert.equal(p?.notes.length, 1);
    assert.ok(p?.notes[0]?.includes("無糖綠"));
    assert.deepEqual(p?.habits, []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
