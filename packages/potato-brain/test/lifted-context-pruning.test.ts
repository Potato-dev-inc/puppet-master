// Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7, file packages/adapters/src/context-pruning.test.ts.
// Copyright 2026 Rakazo contributors. Licensed under the Apache License, Version 2.0;
// see packages/potato-sandbox/LICENSE-APACHE-2.0. Changed by Puppet Master: vitest replaced by node:test; expect matchers
// rewritten as node:assert; cases for dropRepeatedFrameImages added.

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  dropRepeatedFrameImages,
  normalizedImageLimit,
  pruneComputerScreenshotContext,
  pruneForRebuild,
  pruneStalePageStateContext,
} from "../src/lifted/context-pruning.ts";
import type { PromptPart, PruneMessage, ToolResultMessage } from "../src/lifted/context-pruning.ts";

const NOTE_FRAGMENT = "trimmed to save context";

function pageResult(id: string, toolName: string, text = `page ${id} ${"x".repeat(1_200)}`): ToolResultMessage {
  return {
    role: "toolResult",
    toolCallId: id,
    toolName,
    content: [{ type: "text", text }],
    details: { frameId: id },
    isError: false,
    timestamp: 1,
  };
}

function screenshot(frameId: string, toolName = "computer_observe"): ToolResultMessage {
  return {
    role: "toolResult",
    toolCallId: frameId,
    toolName,
    content: [
      { type: "text", text: frameId },
      { type: "image", data: frameId, mimeType: "image/png" },
    ],
    details: { frameId },
    isError: false,
    timestamp: 1,
  };
}

function userImage(name: string): PruneMessage {
  return {
    role: "user",
    content: [{ type: "image", data: name, mimeType: "image/png" }],
    timestamp: 1,
  };
}

function partsOf(message: PruneMessage): PromptPart[] {
  return Array.isArray(message.content) ? message.content : [];
}

/** Whether each message still carries an image part. */
function hasImage(messages: PruneMessage[]): boolean[] {
  return messages.map((message) => partsOf(message).some((part) => part.type === "image"));
}

function textOf(message: PruneMessage): string {
  if (typeof message.content === "string") return message.content;
  return partsOf(message)
    .map((part) => (part.type === "text" ? part.text : part.type))
    .join("");
}

test("trims page-state results older than the three most recent", () => {
  const messages: PruneMessage[] = [
    { role: "user", content: "find the cheapest coffee", timestamp: 1 },
    pageResult("s1", "browser_snapshot"),
    pageResult("s2", "browser_act"),
    {
      role: "toolResult",
      toolCallId: "read-1",
      toolName: "read_file",
      content: [{ type: "text", text: "tracker contents" }],
      isError: false,
      timestamp: 1,
    },
    pageResult("s3", "computer_observe"),
    pageResult("s4", "browser_snapshot"),
    pageResult("s5", "browser_navigate"),
  ];

  const pruned = pruneStalePageStateContext(messages);
  const texts = pruned.map(textOf);
  assert.equal(texts[0], "find the cheapest coffee");
  assert.ok(texts[1]?.includes(NOTE_FRAGMENT));
  assert.ok(texts[2]?.includes(NOTE_FRAGMENT));
  assert.equal(texts[3], "tracker contents");
  assert.ok(texts[4]?.includes("page s3"));
  assert.ok(texts[5]?.includes("page s4"));
  assert.ok(texts[6]?.includes("page s5"));
  // The original history is untouched, so the next request trims identically.
  assert.ok(textOf(messages[1]!).includes("page s1"));
  assert.equal((pruned[1] as ToolResultMessage).toolCallId, "s1");
});

test("neither trims nor counts small page-state results such as errors and receipts", () => {
  const messages: PruneMessage[] = [
    pageResult("s1", "browser_snapshot"),
    pageResult("nav", "browser_navigate", '{"url":"https://example.test","title":"Example"}'),
    pageResult("err", "browser_act", 'Unknown element ref "e9". Call browser_snapshot.'),
    pageResult("s2", "browser_snapshot"),
    pageResult("s3", "browser_snapshot"),
  ];

  assert.equal(pruneStalePageStateContext(messages), messages);
  const keepTwo = pruneStalePageStateContext(messages, 2);
  assert.ok(textOf(keepTwo[0]!).includes(NOTE_FRAGMENT));
  assert.deepEqual(keepTwo.slice(1, 3), messages.slice(1, 3));
});

test("returns the same history when nothing is stale", () => {
  const messages: PruneMessage[] = ["s1", "s2", "s3"].map((id) => pageResult(id, "browser_snapshot"));
  assert.equal(pruneStalePageStateContext(messages), messages);
});

for (const kind of ["thrown", "returned"] as const) {
  test(`does not count a long ${kind} error as a fresh page`, () => {
    const base = pageResult("error", "browser_act", `Action failed: ${"diagnostic ".repeat(150)}`);
    const failure: ToolResultMessage =
      kind === "thrown"
        ? { ...base, isError: true }
        : { ...base, details: { error: "action failed" } };
    const messages: PruneMessage[] = [
      pageResult("s1", "browser_snapshot"),
      pageResult("s2", "computer_observe"),
      pageResult("s3", "browser_snapshot"),
      failure,
    ];
    assert.equal(pruneStalePageStateContext(messages), messages);
  });

  test(`keeps an old long ${kind} error while trimming stale pages`, () => {
    const base = pageResult("error", "computer_act", `Action failed: ${"diagnostic ".repeat(150)}`);
    const failure: ToolResultMessage =
      kind === "thrown"
        ? { ...base, isError: true }
        : { ...base, details: { error: "action failed" } };
    const messages: PruneMessage[] = [
      failure,
      ...["s1", "s2", "s3", "s4"].map((id) => pageResult(id, "browser_snapshot")),
    ];
    const pruned = pruneStalePageStateContext(messages);
    assert.equal(pruned[0], failure);
    assert.ok(textOf(pruned[1]!).includes(NOTE_FRAGMENT));
    assert.deepEqual(pruned.slice(2), messages.slice(2));
    assert.ok(textOf(messages[1]!).includes("page s1"));
  });
}

test("keeps the two latest computer screenshots by default", () => {
  const messages: PruneMessage[] = ["frame-1", "frame-2", "frame-3"].map((id) => screenshot(id));
  assert.deepEqual(hasImage(pruneComputerScreenshotContext(messages)), [false, true, true]);
});

test("honors a model-specific one-image limit", () => {
  const messages: PruneMessage[] = ["frame-1", "frame-2", "frame-3"].map((id) => screenshot(id));
  assert.deepEqual(hasImage(pruneComputerScreenshotContext(messages, 1)), [false, false, true]);
});

test("reserves the image budget for user attachments", () => {
  const messages: PruneMessage[] = [
    {
      role: "user",
      content: [{ type: "image", data: "user-image", mimeType: "image/png" }],
      timestamp: 1,
    },
    screenshot("frame-1"),
    screenshot("frame-2"),
  ];
  assert.deepEqual(hasImage(pruneComputerScreenshotContext(messages, 2)), [true, false, true]);
});

test("rejects user images that exceed a configured model limit", () => {
  const messages: PruneMessage[] = [
    {
      role: "user",
      content: [
        { type: "image", data: "user-image-1", mimeType: "image/png" },
        { type: "image", data: "user-image-2", mimeType: "image/png" },
      ],
      timestamp: 1,
    },
  ];
  assert.throws(() => pruneComputerScreenshotContext(messages, 1), /the prompt contains 2 non-screenshot images/);
});

test("does not apply the default screenshot retention to user images", () => {
  const user: PruneMessage = {
    role: "user",
    content: [
      { type: "image", data: "user-image-1", mimeType: "image/png" },
      { type: "image", data: "user-image-2", mimeType: "image/png" },
      { type: "image", data: "user-image-3", mimeType: "image/png" },
    ],
    timestamp: 1,
  };
  const messages: PruneMessage[] = [user, screenshot("frame-1"), screenshot("frame-2"), screenshot("frame-3")];
  const pruned = pruneComputerScreenshotContext(messages);
  assert.equal(pruned[0], user);
  assert.equal(partsOf(pruned[0]!).length, 3);
  assert.deepEqual(hasImage(pruned.slice(1)), [false, true, true]);
});

test("reuses the message array when no screenshot needs pruning", () => {
  const messages: PruneMessage[] = [
    { role: "user", content: [{ type: "text", text: "hello" }], timestamp: 1 },
  ];
  assert.equal(pruneComputerScreenshotContext(messages), messages);
});

test("keeps two screenshots on a rebuild even when the model allows more", () => {
  const messages = ["a", "b", "c", "d", "e"].map((id) => screenshot(id));
  assert.deepEqual(hasImage(pruneComputerScreenshotContext(messages, 5)), [false, false, false, true, true]);
});

test("drops every screenshot under a zero-image limit and rejects any user image", () => {
  assert.deepEqual(hasImage(pruneComputerScreenshotContext(["a", "b"].map((id) => screenshot(id)), 0)), [false, false]);
  assert.throws(() => pruneComputerScreenshotContext([userImage("u")], 0), /image limit is 0/);
});

test("accepts user images exactly at the limit and rejects one more", () => {
  const atLimit: PruneMessage[] = [userImage("u1"), userImage("u2"), screenshot("a")];
  assert.deepEqual(hasImage(pruneComputerScreenshotContext(atLimit, 2)), [true, true, false]);
  assert.throws(
    () => pruneComputerScreenshotContext([...atLimit, userImage("u3")], 2),
    /contains 3 non-screenshot images/,
  );
});

test("treats a non-finite limit as no limit", () => {
  const messages: PruneMessage[] = [
    userImage("u1"),
    userImage("u2"),
    userImage("u3"),
    ...["a", "b", "c"].map((id) => screenshot(id)),
  ];
  assert.deepEqual(hasImage(pruneComputerScreenshotContext(messages, Number.POSITIVE_INFINITY)), [
    true,
    true,
    true,
    false,
    true,
    true,
  ]);
});

for (const [limit, expected] of [
  [undefined, undefined],
  [0, 0],
  [-3, 0],
  [1, 1],
  [2.9, 2],
  [1000, 1000],
  [Number.NaN, undefined],
  [Number.POSITIVE_INFINITY, undefined],
] as const) {
  test(`reads an image limit of ${String(limit)} as ${String(expected)}`, () => {
    assert.equal(normalizedImageLimit(limit), expected);
  });
}

test("prunes stale page state and screenshots together on a rebuild", () => {
  const page = (id: string): PruneMessage => pageResult(id, "browser_snapshot");
  const messages: PruneMessage[] = [
    page("p1"),
    page("p2"),
    page("p3"),
    page("p4"),
    ...["a", "b", "c"].map((id) => screenshot(id)),
  ];
  const pruned = pruneForRebuild(messages);
  assert.ok(textOf(pruned[0]!).includes("Earlier page state trimmed"));
  assert.ok(!JSON.stringify(pruned.slice(1, 4)).includes("Earlier page state trimmed"));
  assert.deepEqual(hasImage(pruned.slice(4)), [false, true, true]);
  assert.deepEqual(pruneForRebuild(messages.slice(1, 4)), messages.slice(1, 4));
});

test("identical consecutive frames lose their image bytes but keep the metadata text", () => {
  const messages: PruneMessage[] = [screenshot("f1"), screenshot("f1"), screenshot("f2")];
  const deduped = dropRepeatedFrameImages(messages);
  assert.deepEqual(hasImage(deduped), [true, false, true]);
  assert.equal(textOf(deduped[1]!), "f1");
  // The input array is untouched.
  assert.deepEqual(hasImage(messages), [true, true, true]);
});

test("a repeated frame that is not consecutive keeps its image", () => {
  const messages: PruneMessage[] = [screenshot("f1"), screenshot("f2"), screenshot("f1")];
  assert.equal(dropRepeatedFrameImages(messages), messages);
});

test("a rebuild keeps the latest copy of a repeated frame, not every copy", () => {
  // Four screenshots of one frame, then a new frame. The default keeps the two latest images.
  const messages: PruneMessage[] = [
    screenshot("f1"),
    screenshot("f1"),
    screenshot("f1"),
    screenshot("f1"),
    screenshot("f2"),
  ];
  // The count keeps the two latest images: the fourth f1 and f2. The earlier f1 copies lose theirs.
  assert.deepEqual(hasImage(pruneForRebuild(messages)), [false, false, false, true, true]);

  const twoOfOne: PruneMessage[] = [screenshot("f1"), screenshot("f1")];
  assert.deepEqual(hasImage(pruneForRebuild(twoOfOne)), [true, false]);
});

test("a duplicate keeps its image when the earlier copy was pruned by the count", () => {
  // Keep-count leaves only the last image; the earlier f1 loses it, so the later f1 must keep its own.
  const messages: PruneMessage[] = [screenshot("f1"), screenshot("f1")];
  assert.deepEqual(hasImage(pruneComputerScreenshotContext(messages, 1)), [false, true]);
  assert.deepEqual(hasImage(dropRepeatedFrameImages(pruneComputerScreenshotContext(messages, 1))), [false, true]);
});
