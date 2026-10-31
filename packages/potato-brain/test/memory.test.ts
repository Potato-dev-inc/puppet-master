import { test } from "node:test";
import assert from "node:assert/strict";
import { tokenize } from "../src/memory/tokenize.ts";
import { MemoryIndex } from "../src/memory/index.ts";

test("tokenize splits latin runs and adds CJK bigrams", () => {
  const tokens = tokenize("Hello, World 白飯很好吃");
  assert.ok(tokens.includes("hello"));
  assert.ok(tokens.includes("world"));
  assert.equal(tokens.includes("hello,"), false);
  for (const bigram of ["白飯", "飯很", "很好", "好吃"]) {
    assert.ok(tokens.includes(bigram), bigram);
  }
  assert.equal(new Set(tokens).size, tokens.length);
  assert.ok(tokenize("魚").includes("魚"));
});

test("search finds the right line number", () => {
  const index = new MemoryIndex();
  index.add({
    path: "journal/a.md",
    text: "# 日誌\r\n今天吃了拉麵\r\n明天想吃壽司",
    mtime: 1000,
  });

  const hits = index.search("拉麵");
  assert.equal(hits[0]?.path, "journal/a.md");
  assert.equal(hits[0]?.line, 2);
  assert.ok(hits[0]?.why.startsWith("matched: "));
});

test("Chinese query finds a Chinese document", () => {
  const index = new MemoryIndex();
  index.add({ path: "notes/zh.md", text: "白飯很好吃", mtime: 1 });
  index.add({ path: "notes/en.md", text: "the rice was good", mtime: 1 });

  const hits = index.search("白飯");
  assert.ok(hits.length > 0);
  assert.equal(hits[0]?.path, "notes/zh.md");
});

test("add twice replaces rather than duplicates", () => {
  const index = new MemoryIndex();
  index.add({ path: "same.md", text: "alpha beta", mtime: 1 });
  index.add({ path: "same.md", text: "gamma delta", mtime: 2 });

  assert.equal(index.docCount, 1);
  assert.equal(index.search("alpha").length, 0);
  assert.equal(index.search("gamma")[0]?.path, "same.md");
});

test("toJSON/fromJSON round-trips and still finds the same hit", () => {
  const index = new MemoryIndex();
  index.add({
    path: "journal/2026/10/2026-10-28.md",
    text: "# 日誌\n今天吃了白飯\n明天想吃壽司",
    mtime: 2000,
  });
  index.add({ path: "notes/en.md", text: "the rice was good today", mtime: 1500 });

  const before = index.search("白飯");
  const restored = MemoryIndex.fromJSON(JSON.parse(JSON.stringify(index.toJSON())));

  assert.equal(restored.docCount, index.docCount);
  assert.deepEqual(restored.search("白飯"), before);
  assert.deepEqual(restored.search("rice"), index.search("rice"));
});

test("empty query returns no hits", () => {
  const index = new MemoryIndex();
  index.add({ path: "a.md", text: "apple banana", mtime: 1 });

  assert.deepEqual(index.search(""), []);
  assert.deepEqual(index.search("   \n\t "), []);
  assert.deepEqual(new MemoryIndex().search("apple"), []);
});

test("ranking prefers the clearly better match", () => {
  const index = new MemoryIndex();
  index.add({ path: "a.md", text: "apple banana cherry", mtime: 100 });
  index.add({ path: "b.md", text: "apple", mtime: 100 });

  const hits = index.search("apple");
  assert.equal(hits[0]?.path, "b.md");
  assert.equal(hits[0]?.why, "matched: apple");
  assert.ok((hits[0]?.score ?? 0) > (hits[1]?.score ?? 0));
});
