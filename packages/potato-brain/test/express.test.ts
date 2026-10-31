import { test } from "node:test";
import assert from "node:assert/strict";
import { parseExpression, shouldVoice, splitMessage, toneForMood } from "../src/persona/express.ts";
import { isFeel, pickKaomoji } from "../src/persona/kaomoji.ts";
import { seedOf } from "../src/core/hash.ts";

test("parses a trailing state block and strips it", () => {
  const raw = `欸嘿，才不是等你 (๑•̀ㅂ•́)و✧\n::state {"mood":"genki","intensity":3,"feel":"proud","segments":2,"tone":"energetic"}`;
  const { text, expression } = parseExpression(raw);
  assert.equal(text, "欸嘿，才不是等你 (๑•̀ㅂ•́)و✧");
  assert.equal(expression.mood, "genki");
  assert.equal(expression.intensity, 3);
  assert.equal(expression.feel, "proud");
  assert.equal(expression.segments, 2);
  assert.equal(expression.tone, "energetic");
});

test("no marker means no expression and untouched text", () => {
  const { text, expression } = parseExpression("就只是一句話。");
  assert.equal(text, "就只是一句話。");
  assert.deepEqual(expression, {});
});

test("malformed json degrades gracefully", () => {
  const { text, expression } = parseExpression(`哈囉\n::state {oops}`);
  assert.equal(text, "哈囉");
  assert.deepEqual(expression, {});
});

test("unknown enum values are dropped, not passed through", () => {
  const { expression } = parseExpression(`嗨\n::state {"mood":"angry","feel":"bored","segments":9}`);
  assert.equal(expression.mood, undefined);
  assert.equal(expression.feel, undefined);
  assert.equal(expression.segments, undefined);
});

test("splitMessage never cuts a sentence in half", () => {
  const parts = splitMessage("早安呀。今天也要起來喔。我先滾一下。", 2);
  assert.equal(parts.length, 2);
  assert.ok(parts[0]?.endsWith("。"));
  assert.equal(parts.join(""), "早安呀。今天也要起來喔。我先滾一下。");
});

test("splitMessage collapses when there is nothing to split", () => {
  assert.deepEqual(splitMessage("嗯。", 3), ["嗯。"]);
  assert.deepEqual(splitMessage("", 2), [""]);
});

test("voice lottery is deterministic per seed and respects length", () => {
  const policy = { enabled: true, probability: 0.5, maxChars: 60 };
  const seed = seedOf("msg-1");
  const a = shouldVoice("晚安啦。", seed, policy, true);
  const b = shouldVoice("晚安啦。", seed, policy, true);
  assert.equal(a, b);
  assert.equal(shouldVoice("字".repeat(200), seed, policy, true), false);
  assert.equal(shouldVoice("晚安啦。", seed, policy, false), false);
  assert.equal(shouldVoice("晚安啦。", seed, { ...policy, enabled: false }, true), false);
});

test("mood maps to prosody", () => {
  assert.equal(toneForMood("sleepy"), "whisper");
  assert.equal(toneForMood("genki"), "energetic");
  assert.equal(toneForMood("tsundere"), "playful");
  assert.equal(toneForMood("hungry"), "softcute");
});

test("kaomoji choice is stable for the same seed", () => {
  const seed = seedOf("turn-7");
  assert.equal(pickKaomoji("sleepy", seed), pickKaomoji("sleepy", seed));
  assert.ok(isFeel("shy"));
  assert.equal(isFeel("bored"), false);
  assert.ok(pickKaomoji("happy", seed).length > 0);
});
