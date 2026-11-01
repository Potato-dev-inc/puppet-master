// The REAL bundled brain (what the app ships as brain.mjs), started the way the app starts it, against a stand-in
// bridge and a stand-in model maker: a "hey" must come back as a reply. Nothing here is the fake brain
// (test/fake-brain.mjs, which the e2e uses), and nothing needs a key or a network.
//
// Why it exists: the founder's Desktop build sent "hey" and never got an answer; the brain died at once, five times,
// and the app said "brain stopped. Press Start again." with no reason. The e2e passed because it runs the fake brain.
// This test runs the real file with the app's exact environment (an allowlist, nothing inherited: potato/env.rs),
// a bot folder shaped like the app makes it (persona seeded from the bundle, vault, state, an empty share folder,
// no per-bot settings or `instructions.md`, or both) and a send shaped like the app's (`capabilities`, `context`).
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import type { ChildProcess } from "node:child_process";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { startMockBridge } from "./helpers/mock-bridge.ts";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..", "..", "..");
const STUB = pathToFileURL(join(HERE, "helpers", "stub-deepseek.mjs")).href;

/** Builds the brain with the app's own script into a folder of its own, so a parallel test run cannot trip over it. */
function buildBundle(into: string): string {
  const build = spawnSync(process.execPath, [join(ROOT, "scripts", "bundle-potato-brain.mjs")], {
    cwd: ROOT,
    encoding: "utf8",
    timeout: 60000,
    env: { ...process.env, POTATO_BRAIN_OUT: into },
  });
  assert.equal(build.status, 0, build.stderr);
  assert.ok(existsSync(join(into, "brain.mjs")), "brain.mjs was built");
  return into;
}

interface Run {
  bridge: Awaited<ReturnType<typeof startMockBridge>>;
  child: ChildProcess;
  stderr: () => string;
  exit: Promise<number | null>;
  modelLog: string;
  dir: string;
}

/** The environment of potato/env.rs for the DeepSeek brain: an allowlist, then the brain's own settings. */
function appEnv(bot: string, folders: Record<string, string>, url: string, token: string, modelLog: string): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = {};
  for (const name of ["PATH", "Path", "SystemRoot", "WINDIR", "TEMP", "TMP"]) {
    const value = process.env[name];
    if (value !== undefined) env[name] = value;
  }
  return {
    ...env,
    FISHD_PROVIDER: "deepseek",
    FISHD_VAULT: folders.vault,
    FISHD_PERSONA_DIR: folders.persona,
    FISHD_STATE_DIR: folders.state,
    FISHD_PACK: "fish",
    FISHD_VOICE: "0",
    FISHD_GATE: "heuristic",
    FISHD_SHARE_DIR: folders.share,
    DEEPSEEK_API_KEY: "sk-test-not-a-real-key-0123456789",
    PM_POTATO_URL: url,
    PM_POTATO_TOKEN: token,
    PM_POTATO_BOT: bot,
    // the stand-in model maker (test/helpers/stub-deepseek.mjs) reads these two
    STUB_DEEPSEEK_LOG: modelLog,
    STUB_DEEPSEEK_REPLY: "hi there, it is Mochi",
  };
}

/** A bot folder as the app makes it (`brain/<bot>/{vault,vault/state,share-empty,persona}`), persona seeded from the bundle. */
function botFolder(bundle: string, withInstructions: boolean): { dir: string; folders: Record<string, string> } {
  const dir = mkdtempSync(join(tmpdir(), "potato-real-brain-"));
  const folders = {
    vault: join(dir, "vault"),
    state: join(dir, "vault", "state"),
    share: join(dir, "share-empty"),
    persona: join(dir, "persona"),
  };
  for (const folder of [folders.vault, folders.state, folders.share]) mkdirSync(folder, { recursive: true });
  cpSync(join(bundle, "persona"), folders.persona, { recursive: true });
  if (withInstructions) writeFileSync(join(folders.persona, "instructions.md"), "Always answer in two short lines.\n", "utf8");
  return { dir, folders };
}

async function start(bundle: string, withInstructions: boolean): Promise<Run> {
  const { dir, folders } = botFolder(bundle, withInstructions);
  const bridge = await startMockBridge("tok-real-brain");
  const modelLog = join(dir, "model.log");
  const child = spawn(process.execPath, ["--import", STUB, join(bundle, "brain.mjs")], {
    cwd: dir,
    env: appEnv("mochi", folders, bridge.url, "tok-real-brain", modelLog),
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  let text = "";
  child.stdout?.on("data", (chunk: Buffer) => (text += chunk.toString("utf8")));
  child.stderr?.on("data", (chunk: Buffer) => (text += chunk.toString("utf8")));
  const exit = new Promise<number | null>((resolve) => child.once("exit", resolve));
  return { bridge, child, stderr: () => text, exit, modelLog, dir };
}

async function finish(run: Run): Promise<void> {
  if (run.child.exitCode === null) run.child.kill();
  await run.bridge.close();
  rmSync(run.dir, { recursive: true, force: true });
}

/** What the app puts in a `send` (potato/protocol.rs `send_item`, then `with_capabilities`). */
function send(id: string, text: string, sandbox: boolean): Record<string, unknown> {
  return {
    type: "send",
    id,
    conversationId: "app:dm:mochi",
    surface: "app",
    from: { id: "owner", name: "Pat", trust: "owner_pc" },
    text,
    tools: "full",
    context: "Pat is on the desktop app.",
    capabilities: { sandbox },
  };
}

const bundleDir = mkdtempSync(join(tmpdir(), "potato-real-bundle-"));
const BUNDLE = buildBundle(bundleDir);

for (const [label, withInstructions, sandbox] of [
  ["a new bot (no instructions, no computer)", false, false],
  ["a bot with its own instructions and the computer on", true, true],
] as const) {
  test(`the real bundled brain answers "hey": ${label}`, async () => {
    const run = await start(BUNDLE, withInstructions);
    try {
      const hello = await run.bridge.waitFor((e) => e["type"] === "hello", 10000).catch((error: Error) => {
        throw new Error(`${error.message}\nexit=${run.child.exitCode}\nstderr:\n${run.stderr()}`);
      });
      assert.deepEqual(hello["packs"], ["fish", "potato"]);
      assert.equal(hello["provider"], "deepseek", "a key was given, so the brain talks to the model maker");

      run.bridge.push(send("m1", "hey", sandbox));
      const reply = await run.bridge.waitFor((e) => e["type"] === "reply" || e["type"] === "error", 10000).catch((error: Error) => {
        throw new Error(`${error.message}\nexit=${run.child.exitCode}\nstderr:\n${run.stderr()}`);
      });
      assert.equal(reply["type"], "reply", `the brain failed the turn: ${JSON.stringify(reply)}\nstderr:\n${run.stderr()}`);
      assert.equal(reply["inReplyTo"], "m1");
      assert.deepEqual(reply["segments"], ["hi there, it is Mochi"]);

      // It really asked the (stand-in) model maker, once, with a key.
      const asked = readFileSync(run.modelLog, "utf8").trim().split("\n").map((line) => JSON.parse(line) as Record<string, unknown>);
      assert.equal(asked.length, 1, JSON.stringify(asked));
      assert.equal(asked[0]?.["url"], "https://api.deepseek.com/chat/completions");
      assert.equal(asked[0]?.["bearer"], true);

      // A second message on the same conversation works as well, and the process is still the same one.
      run.bridge.push(send("m2", "you there?", sandbox));
      await run.bridge.waitFor((e) => e["type"] === "reply" && e["inReplyTo"] === "m2", 10000);
      assert.equal(run.child.exitCode, null, "the brain is still running");

      run.bridge.push({ type: "shutdown" });
      assert.equal(await run.exit, 0, run.stderr());
      assert.match(run.stderr(), /ready/);
      assert.doesNotMatch(run.stderr(), /could not start|Error/);
    } finally {
      await finish(run);
    }
  });
}

test("the bundle never needs the repo: it runs from a folder with nothing around it", () => {
  // The app runs brain.mjs from its resource folder with no node_modules anywhere near.
  const bare = mkdtempSync(join(tmpdir(), "potato-real-bare-"));
  try {
    cpSync(BUNDLE, join(bare, "potato-brain"), { recursive: true });
    const run = spawnSync(process.execPath, [join(bare, "potato-brain", "brain.mjs")], {
      cwd: bare,
      env: {},
      encoding: "utf8",
      timeout: 20000,
    });
    // started without its environment: it says so and exits with the "not configured" code, not a module error
    assert.equal(run.status, 2, run.stderr);
    assert.match(run.stderr, /not set/);
  } finally {
    rmSync(bare, { recursive: true, force: true });
  }
});

test.after(() => rmSync(bundleDir, { recursive: true, force: true }));
