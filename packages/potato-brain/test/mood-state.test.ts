import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { loadMood, saveMood } from "../src/persona/persona.ts";
import { initialMood } from "../src/persona/mood.ts";

test("an active mood override survives a reload", () => {
  const root = mkdtempSync(join(tmpdir(), "fishd-mood-"));
  try {
    const now = new Date("2026-10-30T12:00:00Z");
    saveMood(root, { ...initialMood(now), mood: "tsundere", overrideTurns: 2 });

    const loaded = loadMood(root, now);
    assert.equal(loaded.mood, "tsundere");
    assert.equal(loaded.overrideTurns, 2);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
