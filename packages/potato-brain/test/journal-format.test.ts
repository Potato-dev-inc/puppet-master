import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { VaultMemory } from "../src/memory/vaultMemory.ts";
import { listMarkdown, readDoc } from "../src/memory/vault.ts";

function journalOf(root: string): string {
  return readDoc(root, listMarkdown(root).find((p) => p.startsWith("journal/")) ?? "");
}

test("journal groups a speaker's consecutive messages and indents her reply", () => {
  const root = mkdtempSync(join(tmpdir(), "fishd-journal-"));
  try {
    const memory = new VaultMemory(root);
    const base = new Date(2026, 9, 29, 21, 1, 0);

    memory.log({ kind: "heard", text: "我想賣一隻大肥魚", at: base, where: "#bot", who: "輕輕鬆鬆啊", whoId: "2" });
    memory.log({
      kind: "heard",
      text: "感覺可以開源",
      at: new Date(2026, 9, 29, 21, 2, 0),
      where: "#bot",
      who: "輕輕鬆鬆啊",
      whoId: "2",
    });
    memory.log({ kind: "said", text: "我又不是商品。", at: new Date(2026, 9, 29, 21, 2, 30), where: "#bot", mood: "tsundere" });

    const doc = journalOf(root);
    assert.ok(
      doc.includes("（#bot）[[people/2|輕輕鬆鬆啊]]：我想賣一隻大肥魚 ／ 感覺可以開源"),
      doc,
    );
    assert.ok(doc.includes("　我：我又不是商品。（tsundere）"), doc);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a reply breaks the group, and a different speaker starts a new line", () => {
  const root = mkdtempSync(join(tmpdir(), "fishd-journal-"));
  try {
    const memory = new VaultMemory(root);
    const t = (m: number) => new Date(2026, 9, 29, 22, m, 0);

    memory.log({ kind: "heard", text: "aaa", at: t(1), where: "#bot", who: "pat", whoId: "1" });
    memory.log({ kind: "said", text: "嗯。", at: t(1), where: "#bot" });
    memory.log({ kind: "heard", text: "bbb", at: t(2), where: "#bot", who: "pat", whoId: "1" });

    const doc = journalOf(root);
    const heardLines = doc.split("\n").filter((line) => line.startsWith("- "));
    assert.equal(heardLines.length, 2, doc);
    assert.ok(!doc.includes("aaa ／ bbb"), "a reply must end the group");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a gap over 15 minutes starts a new group", () => {
  const root = mkdtempSync(join(tmpdir(), "fishd-journal-"));
  try {
    const memory = new VaultMemory(root);
    memory.log({ kind: "heard", text: "aaa", at: new Date(2026, 9, 29, 10, 0, 0), where: "#bot", who: "pat", whoId: "1" });
    memory.log({ kind: "heard", text: "bbb", at: new Date(2026, 9, 29, 10, 30, 0), where: "#bot", who: "pat", whoId: "1" });

    const doc = journalOf(root);
    assert.equal(doc.split("\n").filter((line) => line.startsWith("- ")).length, 2, doc);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
