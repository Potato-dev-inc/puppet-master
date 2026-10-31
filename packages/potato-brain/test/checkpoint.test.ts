import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { writeCheckpoint } from "../src/memory/checkpoint.ts";
import { StubProvider } from "../src/core/provider.ts";

test("a restart never overwrites an older checkpoint", async () => {
  const root = mkdtempSync(join(tmpdir(), "fishd-cp-"));
  try {
    const provider = new StubProvider(["重點一"]);
    const messages = [{ role: "user" as const, content: "hi" }];

    const first = await writeCheckpoint({ root, scope: "main", index: 1, messages, provider });
    assert.equal(first?.index, 1);
    const before = readFileSync(join(root, "sessions/main/cp-001.md"), "utf8");

    // A fresh process starts its counter at 1 again.
    const again = await writeCheckpoint({ root, scope: "main", index: 1, messages, provider });
    assert.equal(again?.index, 2);
    assert.equal(readFileSync(join(root, "sessions/main/cp-001.md"), "utf8"), before);
    assert.ok(existsSync(join(root, "sessions/main/cp-002.md")));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("an empty transcript writes nothing", async () => {
  const root = mkdtempSync(join(tmpdir(), "fishd-cp-"));
  try {
    const provider = new StubProvider(["重點"]);
    const out = await writeCheckpoint({ root, scope: "main", index: 1, messages: [], provider });
    assert.equal(out, null);
    assert.ok(!existsSync(join(root, "sessions/main/cp-001.md")));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
