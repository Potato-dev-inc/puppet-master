import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  appendJournal,
  appendLine,
  dayKey,
  journalPath,
  listMarkdown,
  readDoc,
  readWindow,
} from "../src/memory/vault.ts";

function withTemp(fn: (root: string) => void): void {
  const root = mkdtempSync(join(tmpdir(), "vault-"));
  try {
    fn(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

const at = (h: number, m = 0) => new Date(2026, 9, 28, h, m, 0, 0);

test("appendJournal creates nested dirs and reports the 1-based entry line", () => {
  withTemp((root) => {
    const date = at(9, 5);
    assert.equal(dayKey(date), "2026-10-28");

    const jot = appendJournal(root, date, "起床");

    assert.equal(jot.path, "journal/2026/10/2026-10-28.md");
    assert.equal(jot.line, 3);
    assert.ok(existsSync(journalPath(root, date)));
    assert.equal(readDoc(root, jot.path), "# 2026-10-28\n\n- 09:05 起床\n");
  });
});

test("appendJournal appends without rewriting prior bytes", () => {
  withTemp((root) => {
    const first = appendJournal(root, at(9, 5), "起床");
    const path = journalPath(root, at(9, 5));
    const before = readFileSync(path);

    const second = appendJournal(root, at(9, 6), "吃早餐");
    const after = readFileSync(path);

    assert.equal(first.line, 3);
    assert.equal(second.line, 4);
    assert.ok(after.subarray(0, before.length).equals(before));
    assert.equal(after.toString("utf8"), "# 2026-10-28\n\n- 09:05 起床\n- 09:06 吃早餐\n");
  });
});

test("readWindow returns exactly the requested lines and clamps", () => {
  withTemp((root) => {
    const rel = "nodes/fact/notes.md";
    for (const line of ["one", "two", "three", "four", "five"]) {
      appendLine(root, rel, line);
    }

    assert.equal(readWindow(root, rel, 2, 2), "two\nthree");
    assert.equal(readWindow(root, rel, 4, 10), "four\nfive");
    assert.equal(readWindow(root, rel, 99, 2), "");
    assert.equal(readWindow(root, rel, 0, 2), "one\ntwo");
    assert.equal(readWindow(root, rel, -5, 1), "one");
    assert.equal(readWindow(root, rel, 1, 0), "");
  });
});

test("listMarkdown returns sorted forward-slash paths and skips persona, state, dots", () => {
  withTemp((root) => {
    appendLine(root, "sessions/s1/cp-001.md", "checkpoint");
    appendLine(root, "journal/2026/10/2026-10-28.md", "entry");
    appendLine(root, "nodes/person/c.md", "c");
    appendLine(root, "nodes/fact/b.md", "b");
    appendLine(root, "nodes/fact/a.md", "a");
    appendLine(root, "persona/core.md", "persona");
    appendLine(root, "state/mood.json", "state");
    appendLine(root, "journal/.obsidian/hidden.md", "hidden");

    assert.deepEqual(listMarkdown(root), [
      "journal/2026/10/2026-10-28.md",
      "nodes/fact/a.md",
      "nodes/fact/b.md",
      "nodes/person/c.md",
      "sessions/s1/cp-001.md",
    ]);
  });
});

test("a path traversal attempt throws", () => {
  withTemp((root) => {
    assert.throws(() => readDoc(root, "../../etc/passwd"), /escapes vault/);
    assert.throws(() => appendLine(root, "../outside.md", "x"), /escapes vault/);
    assert.throws(() => readDoc(root, "/etc/passwd"), /escapes vault/);
  });
});
