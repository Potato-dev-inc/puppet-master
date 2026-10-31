// The shipped form: scripts/bundle-potato-brain.mjs builds brain.mjs and the
// persona folder, and the result runs on its own, with no node_modules.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { startMockBridge } from "./helpers/mock-bridge.ts";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..", "..", "..");
const OUT = join(ROOT, "packages", "app", "src-tauri", "resources", "potato-brain");
const BRAIN = join(OUT, "brain.mjs");

function cleanEnv(extra: Record<string, string> = {}): Record<string, string> {
  const keep: Record<string, string> = {};
  for (const name of ["PATH", "Path", "SystemRoot", "WINDIR", "TEMP", "TMP", "TMPDIR"]) {
    const value = process.env[name];
    if (value !== undefined) keep[name] = value;
  }
  return { ...keep, ...extra };
}

test("the bundle builds without discord.js and with the persona next to it", () => {
  const build = spawnSync(process.execPath, [join(ROOT, "scripts", "bundle-potato-brain.mjs")], {
    cwd: ROOT,
    encoding: "utf8",
    timeout: 60000,
  });
  assert.equal(build.status, 0, build.stderr);
  assert.ok(existsSync(BRAIN));
  assert.ok(!/discord/i.test(readFileSync(BRAIN, "utf8")));
  for (const file of ["protocol.md", "packs/fish/core.md", "packs/fish/rules.md", "packs/potato/core.md"]) {
    assert.ok(existsSync(join(OUT, "persona", file)), file);
  }
  const deps = JSON.parse(readFileSync(join(HERE, "..", "package.json"), "utf8")) as Record<string, unknown>;
  assert.equal("dependencies" in deps, false, "the package has no runtime dependency");
});

test("brain.mjs with no environment exits 2; with one it talks to the bridge", async () => {
  const bare = spawnSync(process.execPath, [BRAIN], { env: cleanEnv(), encoding: "utf8", timeout: 20000 });
  assert.equal(bare.status, 2);
  assert.match(bare.stderr, /not set/);

  const dir = mkdtempSync(join(tmpdir(), "potato-bundle-"));
  const bridge = await startMockBridge("tok-bundle");
  const child = spawn(process.execPath, [BRAIN], {
    cwd: dir,
    stdio: "ignore",
    env: cleanEnv({
      PM_POTATO_URL: bridge.url,
      PM_POTATO_TOKEN: "tok-bundle",
      FISHD_PROVIDER: "stub",
      FISHD_VAULT: join(dir, "vault"),
      FISHD_STATE_DIR: join(dir, "state"),
      FISHD_PERSONA_DIR: join(OUT, "persona"),
      FISHD_PACK: "fish",
      FISHD_VOICE: "0",
    }),
  });
  const exit = new Promise<number | null>((resolve) => child.once("exit", resolve));
  try {
    const hello = await bridge.waitFor((e) => e["type"] === "hello");
    assert.deepEqual(hello["packs"], ["fish", "potato"]);
    bridge.push({
      type: "send",
      id: "m1",
      conversationId: "app:dm:bot1",
      surface: "app",
      from: { id: "owner", name: "Pat", trust: "owner_pc" },
      text: "hello",
      tools: "full",
    });
    await bridge.waitFor((e) => e["type"] === "reply");
    bridge.push({ type: "shutdown" });
    assert.equal(await exit, 0);
  } finally {
    if (child.exitCode === null) child.kill();
    await bridge.close();
    rmSync(dir, { recursive: true, force: true });
  }
});
