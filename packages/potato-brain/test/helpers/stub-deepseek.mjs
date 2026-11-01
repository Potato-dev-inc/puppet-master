// Preloaded with `node --import` into the REAL bundled brain (test/real-brain.test.ts). The brain talks to
// https://api.deepseek.com with the global fetch and has no setting for the address, so this stands in for the
// model maker: a call to it is answered here (a short canned reply, or a tool call when asked) and written to
// STUB_DEEPSEEK_LOG as one JSON line; every other address goes to the real fetch (the brain only talks to the
// local bridge). Nothing leaves the machine, and no key is needed.
import { appendFileSync } from "node:fs";

const real = globalThis.fetch;
const logFile = process.env.STUB_DEEPSEEK_LOG ?? "";
const reply = process.env.STUB_DEEPSEEK_REPLY ?? "hi there";

globalThis.fetch = async (input, init) => {
  const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
  if (!url.startsWith("https://api.deepseek.com")) return real(input, init);
  let body = {};
  try {
    body = JSON.parse(String(init?.body ?? "{}"));
  } catch {
    // a body that is not JSON is logged as empty
  }
  const auth = new Headers(init?.headers).get("authorization") ?? "";
  if (logFile !== "") {
    appendFileSync(
      logFile,
      `${JSON.stringify({ url, model: body.model, messages: (body.messages ?? []).length, tools: (body.tools ?? []).length, bearer: auth.startsWith("Bearer ") })}\n`,
    );
  }
  return new Response(
    JSON.stringify({
      choices: [{ message: { role: "assistant", content: reply }, finish_reason: "stop" }],
      usage: { prompt_tokens: 120, completion_tokens: 4, prompt_cache_hit_tokens: 0, prompt_cache_miss_tokens: 120 },
    }),
    { status: 200, headers: { "content-type": "application/json" } },
  );
};
