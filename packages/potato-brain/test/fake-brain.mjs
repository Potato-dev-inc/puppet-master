#!/usr/bin/env node
// A brain with no model, no persona and no dependencies. It speaks channel
// protocol v1 (see src/channels/potato.ts) so Puppet Master can be tested
// against a real child process without a key, a network or a vault.
//
//   hello                      first, once
//   send "<text>"              typing on, reply "Fish heard: <text>", mood genki,
//                              usage 10/5 (prompt/completion), typing off
//   send "work: <task>"        the same, plus a helper_request for <task>
//   send "crash"               exits with code 1 at once
//   helper_update (not started) a reply "Fish heard: helper <status>"
//   shutdown                   exits with code 0
//   anything else              ignored
//
// Exit codes: 2 without PM_POTATO_URL and PM_POTATO_TOKEN, 3 when the app
// keeps refusing it or does not answer.

const url = (process.env.PM_POTATO_URL ?? "").trim();
const token = (process.env.PM_POTATO_TOKEN ?? "").trim();
if (url === "" || token === "") {
  console.error("fake-brain: PM_POTATO_URL and PM_POTATO_TOKEN are required");
  process.exit(2);
}

const base = url.endsWith("/") ? url : `${url}/`;
const waitMs = Number(process.env.PM_FAKE_WAIT_MS ?? 25000);
const headers = { "X-PM-Potato-Token": token };
let cursor = 0;
let counter = 0;
let failures = 0;
const helpers = new Map();

async function post(events) {
  const res = await fetch(new URL("potato/brain/events", base), {
    method: "POST",
    headers: { ...headers, "Content-Type": "application/json" },
    body: JSON.stringify({ v: 1, events: events.map((e) => ({ v: 1, ...e })) }),
    redirect: "error",
  });
  await res.arrayBuffer().catch(() => {});
  if (!res.ok) throw new Error(`events ${res.status}`);
}

async function handle(item) {
  if (!item || item.v !== 1) return;

  if (item.type === "shutdown") process.exit(0);

  if (item.type === "send") {
    const text = String(item.text ?? "");
    if (text.trim() === "crash") process.exit(1);
    const conversationId = String(item.conversationId);
    const events = [{ type: "typing", conversationId, on: true }];
    const work = /^work:\s*(.+)$/is.exec(text.trim());
    if (work) {
      const requestId = `fake-${++counter}`;
      helpers.set(requestId, conversationId);
      events.push({
        type: "helper_request",
        requestId,
        conversationId,
        task: (work[1] ?? "").slice(0, 600),
      });
    }
    events.push(
      { type: "reply", conversationId, inReplyTo: item.id, segments: [`Fish heard: ${text}`] },
      { type: "mood", mood: "genki", intensity: 2, energy: 60, hunger: 30 },
      { type: "usage", model: "fake", promptTokens: 10, completionTokens: 5, cacheHitTokens: 0, cacheMissTokens: 10 },
      { type: "typing", conversationId, on: false },
    );
    await post(events);
    return;
  }

  if (item.type === "helper_update" && item.status !== "started") {
    const conversationId = helpers.get(item.requestId);
    if (!conversationId) return;
    helpers.delete(item.requestId);
    await post([
      { type: "reply", conversationId, inReplyTo: item.requestId, segments: [`Fish heard: helper ${item.status}`] },
    ]);
  }
}

async function main() {
  await post([
    {
      type: "hello",
      protocol: 1,
      version: "fake",
      provider: "stub",
      packs: ["fish", "potato"],
      surfaces: ["app"],
    },
  ]);

  for (;;) {
    try {
      const res = await fetch(new URL(`potato/brain/inbox?cursor=${cursor}&wait_ms=${waitMs}`, base), {
        headers,
        redirect: "error",
      });
      if (!res.ok) throw new Error(`inbox ${res.status}`);
      const body = await res.json();
      failures = 0;
      if (typeof body.cursor === "number") cursor = body.cursor;
      const items = Array.isArray(body.items) ? body.items : [];
      for (const item of items) await handle(item);
      if (items.length === 0) await new Promise((r) => setTimeout(r, 100));
    } catch {
      if (++failures >= 20) {
        console.error("fake-brain: the app does not answer");
        process.exit(3);
      }
      await new Promise((r) => setTimeout(r, 250));
    }
  }
}

main().catch(() => {
  console.error("fake-brain: could not say hello");
  process.exit(3);
});
