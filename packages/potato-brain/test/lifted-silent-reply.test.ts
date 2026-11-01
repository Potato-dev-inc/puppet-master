// Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7, file packages/adapters/src/silent-reply.ts (no upstream test file for this module; cases are written from its doc comments).
// Copyright 2026 Rakazo contributors. Licensed under the Apache License, Version 2.0;
// see packages/potato-sandbox/LICENSE-APACHE-2.0. Changed by Puppet Master: focused node:test cases written for the local ReplyBlock type.

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  NO_RESPONSE,
  PEER_NOTHING_NEW_REPLY,
  isExactNoResponse,
  peerReplyWithoutSentinel,
  stripNoResponseReply,
  stripStrayNoResponse,
} from "../src/lifted/silent-reply.ts";
import type { ReplyBlock } from "../src/lifted/silent-reply.ts";

const text = (value: string): ReplyBlock => ({ kind: "text", text: value });

test("the exact token, alone, is a silent reply: nothing is posted", () => {
  assert.equal(NO_RESPONSE, "NO_RESPONSE");
  const result = stripNoResponseReply("NO_RESPONSE", [text("NO_RESPONSE")]);
  assert.equal(result.assembled, "");
  assert.deepEqual(result.blocks, []);
  assert.deepEqual(stripNoResponseReply("  NO_RESPONSE \n", []).assembled, "");
});

test("the token repeated, glued or spaced, is still silent", () => {
  assert.equal(isExactNoResponse("NO_RESPONSENO_RESPONSE"), true);
  assert.equal(isExactNoResponse("NO_RESPONSE NO_RESPONSE"), true);
  assert.equal(stripNoResponseReply("NO_RESPONSENO_RESPONSE", [text("NO_RESPONSENO_RESPONSE")]).assembled, "");
});

test("prose around the token is a reply, not a silent one", () => {
  assert.equal(isExactNoResponse("Done. NO_RESPONSE"), false);
  assert.equal(isExactNoResponse("NO_RESPONSE because"), false);
  assert.equal(stripNoResponseReply("Done. NO_RESPONSE", [text("Done. NO_RESPONSE")]).assembled, "Done.");
});

test("a longer identifier that starts with the token is a word, not the token", () => {
  const prose = "NO_RESPONSE_POLICY is the rule";
  assert.equal(isExactNoResponse(prose), false);
  assert.equal(stripStrayNoResponse(prose, [text(prose)]).assembled, prose);
});

test("tool steps do not count as extra prose, and they survive a silent reply", () => {
  const blocks: ReplyBlock[] = [
    { kind: "tool", name: "lookup" },
    { kind: "text", text: "NO_RESPONSE", id: 1 },
  ];
  const result = stripNoResponseReply("", blocks);
  assert.equal(result.assembled, "");
  assert.deepEqual(result.blocks, [{ kind: "tool", name: "lookup" }]);
});

test("an exact final next to a text block with prose keeps the reply (fail closed)", () => {
  const blocks = [text("NO_RESPONSE"), text("Here is the answer")];
  const result = stripNoResponseReply("NO_RESPONSE", blocks);
  assert.equal(result.assembled, "NO_RESPONSE");
  assert.equal(result.blocks, blocks);
});

test("a stray sentinel at the edge of a reply with prose is dropped from the text", () => {
  const result = stripStrayNoResponse("", [text("Sure. NO_RESPONSE")]);
  assert.equal(result.assembled, "");
  assert.deepEqual(result.blocks, [text("Sure.")]);
});

test("a sentinel-only reply is left alone by the stray-sentinel stripper", () => {
  const blocks = [text("NO_RESPONSE")];
  const result = stripStrayNoResponse("NO_RESPONSE", blocks);
  assert.equal(result.assembled, "NO_RESPONSE");
  assert.equal(result.blocks, blocks);
});

test("a peer asking gets the words, not the literal token", () => {
  const result = peerReplyWithoutSentinel("NO_RESPONSE", [text("NO_RESPONSE")]);
  assert.equal(result.assembled, PEER_NOTHING_NEW_REPLY);
  assert.deepEqual(result.blocks, [text(PEER_NOTHING_NEW_REPLY)]);
});
