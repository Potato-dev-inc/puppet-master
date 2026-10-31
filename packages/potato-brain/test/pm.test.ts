// pm.ts and the fake brain as real child processes, on the loopback only.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import type { ChildProcess } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { BRAIN_VERSION } from "../src/version.ts";
import { loadPersona } from "../src/persona/persona.ts";
import { estimateTokens } from "../src/core/budget.ts";
import { startMockBridge } from "./helpers/mock-bridge.ts";

const HERE = dirname(fileURLToPath(import.meta.url));
const PKG = join(HERE, "..");
const PM = join(PKG, "src", "pm.ts");
const FAKE = join(HERE, "fake-brain.mjs");

/** An environment with nothing from the parent but what node itself needs. */
function cleanEnv(extra: Record<string, string> = {}): Record<string, string> {
  const keep: Record<string, string> = {};
  for (const name of ["PATH", "Path", "SystemRoot", "WINDIR", "TEMP", "TMP", "TMPDIR"]) {
    const value = process.env[name];
    if (value !== undefined) keep[name] = value;
  }
  return { ...keep, ...extra };
}

function exited(child: ChildProcess, ms = 10000): Promise<number | null> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      child.kill();
      reject(new Error("the child did not exit"));
    }, ms);
    child.once("exit", (code) => {
      clearTimeout(timer);
      resolve(code);
    });
  });
}

test("package.json and version.ts agree", () => {
  const pkg = JSON.parse(readFileSync(join(PKG, "package.json"), "utf8")) as { version: string };
  assert.equal(pkg.version, BRAIN_VERSION);
});

test("pm.ts with no environment exits 2 with a plain message", () => {
  const run = spawnSync(process.execPath, [PM], { env: cleanEnv(), encoding: "utf8", timeout: 20000 });
  assert.equal(run.status, 2);
  assert.match(run.stderr, /PM_POTATO_URL and PM_POTATO_TOKEN are not set/);
  assert.equal(run.stdout, "");
});

test("pm.ts needs both variables, and an address on this computer", () => {
  const onlyUrl = spawnSync(process.execPath, [PM], {
    env: cleanEnv({ PM_POTATO_URL: "http://127.0.0.1:1" }),
    encoding: "utf8",
    timeout: 20000,
  });
  assert.equal(onlyUrl.status, 2);

  const remote = spawnSync(process.execPath, [PM], {
    env: cleanEnv({ PM_POTATO_URL: "http://example.com:80", PM_POTATO_TOKEN: "t" }),
    encoding: "utf8",
    timeout: 20000,
  });
  assert.equal(remote.status, 2);
  assert.match(remote.stderr, /on this computer/);
  assert.ok(!remote.stderr.includes("example.com"), "the address is not echoed");
});

test("no .env is read when PM_POTATO_URL is set", () => {
  const dir = mkdtempSync(join(tmpdir(), "potato-env-"));
  try {
    writeFileSync(join(dir, ".env"), "FISHD_MODEL=from-dotenv\nFISHD_MAX_TOKENS=111\n");
    const code =
      `const { CONFIG } = await import(${JSON.stringify(pathToFileURL(join(PKG, "src", "config.ts")).href)});` +
      `console.log(JSON.stringify([CONFIG.model, CONFIG.maxTokens]));`;
    const read = (extra: Record<string, string>): unknown =>
      JSON.parse(
        spawnSync(process.execPath, ["--input-type=module", "-e", code], {
          cwd: dir,
          env: cleanEnv(extra),
          encoding: "utf8",
          timeout: 20000,
        }).stdout.trim(),
      );

    assert.deepEqual(read({}), ["from-dotenv", 111], "a plain run still reads .env");
    assert.deepEqual(read({ PM_POTATO_URL: "http://127.0.0.1:1" }), ["deepseek-chat", 600]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("pm.ts runs end to end against a mock bridge with the stub provider", async () => {
  const dir = mkdtempSync(join(tmpdir(), "potato-pm-"));
  const bridge = await startMockBridge("tok-e2e");
  const cwd = join(dir, "brain");
  mkdirSync(cwd, { recursive: true });
  const child = spawn(process.execPath, [PM], {
    cwd,
    stdio: ["ignore", "pipe", "pipe"],
    env: cleanEnv({
      PM_POTATO_URL: bridge.url,
      PM_POTATO_TOKEN: "tok-e2e",
      PM_POTATO_BOT: "bot1",
      FISHD_PROVIDER: "stub",
      FISHD_VAULT: join(dir, "vault"),
      FISHD_STATE_DIR: join(dir, "state"),
      FISHD_PERSONA_DIR: join(PKG, "persona"),
      FISHD_PACK: "fish",
      FISHD_VOICE: "0",
      FISHD_SHARE_DIR: join(dir, "empty"),
    }),
  });
  const out: string[] = [];
  child.stdout?.on("data", (c: Buffer) => out.push(c.toString()));
  child.stderr?.on("data", (c: Buffer) => out.push(c.toString()));
  try {
    const hello = await bridge.waitFor((e) => e["type"] === "hello");
    assert.equal(hello["provider"], "stub");
    assert.deepEqual(hello["packs"], ["fish", "potato"]);
    assert.deepEqual(hello["surfaces"], ["app"]);
    assert.equal(hello["version"], BRAIN_VERSION);

    bridge.push({
      type: "send",
      id: "m1",
      conversationId: "app:dm:bot1",
      surface: "app",
      from: { id: "owner", name: "Pat", trust: "owner_pc" },
      text: "hello",
      tools: "full",
    });
    const reply = await bridge.waitFor((e) => e["type"] === "reply");
    assert.equal(reply["inReplyTo"], "m1");
    await bridge.waitFor((e) => e["type"] === "usage");

    bridge.push({ type: "shutdown" });
    assert.equal(await exited(child), 0);

    const text = out.join("");
    assert.ok(!text.includes("tok-e2e"), "the token is not printed");
    assert.ok(!text.includes(bridge.url), "the address is not printed");
    assert.ok(!text.includes("hello"), "no message text is printed");
  } finally {
    if (child.exitCode === null) child.kill();
    await bridge.close();
    rmSync(dir, { recursive: true, force: true });
  }
});

test("pm.ts with no key says PROVIDER_AUTH instead of answering from the stub", async () => {
  const dir = mkdtempSync(join(tmpdir(), "potato-pm-"));
  const bridge = await startMockBridge("tok-nokey");
  const child = spawn(process.execPath, [PM], {
    cwd: dir,
    stdio: "ignore",
    env: cleanEnv({
      PM_POTATO_URL: bridge.url,
      PM_POTATO_TOKEN: "tok-nokey",
      // no provider, no key, and a home that holds no login file
      HOME: join(dir, "home"),
      USERPROFILE: join(dir, "home"),
      FISHD_VAULT: join(dir, "vault"),
      FISHD_PERSONA_DIR: join(PKG, "persona"),
      FISHD_PACK: "fish",
    }),
  });
  try {
    const hello = await bridge.waitFor((e) => e["type"] === "hello");
    assert.equal(hello["provider"], "none");
    bridge.push({
      type: "send",
      id: "m1",
      conversationId: "app:dm:bot1",
      surface: "app",
      from: { id: "owner", name: "Pat", trust: "owner_pc" },
      text: "hello",
      tools: "full",
    });
    const err = await bridge.waitFor((e) => e["type"] === "error");
    assert.equal(err["code"], "PROVIDER_AUTH");
    bridge.push({ type: "shutdown" });
    assert.equal(await exited(child), 0);
  } finally {
    if (child.exitCode === null) child.kill();
    await bridge.close();
    rmSync(dir, { recursive: true, force: true });
  }
});

test("the fake brain: hello, echo, helper_request, crash, shutdown", async () => {
  const bridge = await startMockBridge("tok-fake");
  const env = cleanEnv({ PM_POTATO_URL: bridge.url, PM_POTATO_TOKEN: "tok-fake", PM_FAKE_WAIT_MS: "60" });
  const child = spawn(process.execPath, [FAKE], { env, stdio: "ignore" });
  try {
    const hello = await bridge.waitFor((e) => e["type"] === "hello");
    assert.equal(hello["protocol"], 1);

    const from = { id: "owner", name: "Pat", trust: "owner_pc" };
    bridge.push({ type: "send", id: "a", conversationId: "app:dm:b", surface: "app", from, text: "hello", tools: "full" });
    const reply = await bridge.waitFor((e) => e["type"] === "reply" && e["inReplyTo"] === "a");
    assert.deepEqual(reply["segments"], ["Fish heard: hello"]);
    const mood = await bridge.waitFor((e) => e["type"] === "mood");
    assert.equal(mood["mood"], "genki");
    const usage = await bridge.waitFor((e) => e["type"] === "usage");
    assert.equal(usage["promptTokens"], 10);
    assert.equal(usage["completionTokens"], 5);
    const typing = bridge.events.filter((e) => e["type"] === "typing").map((e) => e["on"]);
    assert.deepEqual(typing, [true, false]);

    bridge.push({ type: "send", id: "b", conversationId: "app:dm:b", surface: "app", from, text: "work: add a readme", tools: "full" });
    const req = await bridge.waitFor((e) => e["type"] === "helper_request");
    assert.equal(req["task"], "add a readme");
    assert.equal(req["conversationId"], "app:dm:b");

    bridge.push({ type: "helper_update", requestId: req["requestId"], status: "finished", summary: "done" });
    await bridge.waitFor((e) => e["type"] === "reply" && e["inReplyTo"] === req["requestId"]);

    bridge.push({ type: "send", id: "c", conversationId: "app:dm:b", surface: "app", from, text: "crash", tools: "full" });
    assert.equal(await exited(child), 1);
  } finally {
    if (child.exitCode === null) child.kill();
    await bridge.close();
  }

  const second = await startMockBridge("tok-fake2");
  const child2 = spawn(process.execPath, [FAKE], {
    env: cleanEnv({ PM_POTATO_URL: second.url, PM_POTATO_TOKEN: "tok-fake2", PM_FAKE_WAIT_MS: "60" }),
    stdio: "ignore",
  });
  try {
    await second.waitFor((e) => e["type"] === "hello");
    second.push({ type: "shutdown" });
    assert.equal(await exited(child2), 0);
  } finally {
    if (child2.exitCode === null) child2.kill();
    await second.close();
  }
});

test("the fake brain with no environment exits 2", () => {
  const run = spawnSync(process.execPath, [FAKE], { env: cleanEnv(), encoding: "utf8", timeout: 20000 });
  assert.equal(run.status, 2);
});

test("the persona packs fit the static budget and carry the three required rules", () => {
  const dir = join(PKG, "persona");
  for (const pack of ["fish", "potato"]) {
    const tokens = loadPersona(dir, pack).reduce((n, b) => n + estimateTokens(b.content), 0);
    assert.ok(tokens <= 4000, `${pack} persona is ${tokens} tokens`);
  }
  const fishCore = estimateTokens(readFileSync(join(dir, "packs", "fish", "core.md"), "utf8"));
  assert.ok(fishCore < 2500, `fish core.md is ${fishCore} tokens`);

  for (const pack of ["fish", "potato"]) {
    const rules = readFileSync(join(dir, "packs", pack, "rules.md"), "utf8");
    assert.match(rules, /Answer in the language of the person/i);
    assert.match(rules, /Never claim that work has started/i);
    assert.match(rules, /not an official assistant of any company/i);
  }
  // the shared protocol is picked up by every pack
  const blocks = loadPersona(dir, "potato").map((b) => b.id);
  assert.ok(blocks.includes("persona:protocol"));
});

test("no pack names a company, a model or an agent", () => {
  const dir = join(PKG, "persona");
  const files = [
    "protocol.md",
    join("packs", "fish", "core.md"),
    join("packs", "fish", "rules.md"),
    join("packs", "potato", "core.md"),
    join("packs", "potato", "rules.md"),
  ];
  const banned = /deepseek|openai|anthropic|claude|chatgpt|gemini|llama|codex|opencode|kilo|cursor|discord/i;
  for (const file of files) {
    const text = readFileSync(join(dir, file), "utf8");
    assert.ok(!banned.test(text), `${file} names a brand`);
  }
});
