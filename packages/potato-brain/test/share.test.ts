import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { loadShare, pickShare } from "../src/persona/share.ts";

function makeShare(files: string[], index?: string): string {
  const root = mkdtempSync(join(tmpdir(), "fishd-share-"));
  for (const name of files) writeFileSync(join(root, name), "x");
  if (index !== undefined) writeFileSync(join(root, "index.md"), index, "utf8");
  return root;
}

test("index lines become tags and notes", () => {
  const root = makeShare(
    ["cat.gif", "whale.mp4"],
    ["- cat.gif | 貓,笑死 | 貓咪跳失敗", "- whale.mp4 | 白飯 | 大肥魚吃白飯"].join("\n"),
  );
  try {
    const items = loadShare(root);
    const cat = items.find((item) => item.name === "cat.gif");
    assert.deepEqual(cat?.tags, ["貓", "笑死"]);
    assert.equal(cat?.note, "貓咪跳失敗");
    assert.equal(cat?.kind, "image");
    assert.equal(items.find((item) => item.name === "whale.mp4")?.kind, "video");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a file dropped in with no index line still counts", () => {
  const root = makeShare(["mystery.png"]);
  try {
    const items = loadShare(root);
    assert.equal(items.length, 1);
    assert.equal(items[0]?.note, "mystery.png");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a matching tag beats a non-matching file", () => {
  const root = makeShare(
    ["a.gif", "b.gif"],
    ["- a.gif | 白飯 | 吃飯", "- b.gif | 加班,累 | 翻白眼"].join("\n"),
  );
  try {
    const items = loadShare(root);
    assert.equal(pickShare(items, "今天加班好累", 1)?.name, "b.gif");
    assert.equal(pickShare(items, "想吃白飯", 1)?.name, "a.gif");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("nothing matching still picks something, but not the recent one", () => {
  const root = makeShare(["a.gif", "b.gif", "c.gif"]);
  try {
    const items = loadShare(root);
    const picked = pickShare(items, "zzz", 7, ["a.gif", "b.gif"]);
    assert.equal(picked?.name, "c.gif");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("an empty folder yields nothing", () => {
  const root = mkdtempSync(join(tmpdir(), "fishd-share-"));
  try {
    mkdirSync(join(root, "sub"));
    assert.equal(loadShare(root).length, 0);
    assert.equal(pickShare([], "x", 1), null);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
