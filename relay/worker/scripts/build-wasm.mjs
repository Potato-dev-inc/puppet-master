// Rebuild the WebAssembly module (relay/protocol's checks) that the Worker bundles.
//
//   npm run build:wasm
//
// Needs: Rust with the wasm32 target (`rustup target add wasm32-unknown-unknown`) and the
// `wasm-bindgen` CLI at exactly the version pinned in wasm/Cargo.toml
// (`cargo install wasm-bindgen-cli --version 0.2.129 --locked`; point WASM_BINDGEN at it if it is
// not on PATH). The result goes to src/wasm/ and is committed, so deploying the Worker needs no Rust.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const worker = resolve(here, "..");
const relay = resolve(worker, "..");
const outDir = join(worker, "src", "wasm");
const bindgen = process.env.WASM_BINDGEN || "wasm-bindgen";
const cargoHome = process.env.CARGO_HOME || join(homedir(), ".cargo");
const targetDir = process.env.CARGO_TARGET_DIR || join(relay, "target");

function run(cmd, args, options = {}) {
  const result = spawnSync(cmd, args, { stdio: "inherit", ...options });
  if (result.status !== 0) {
    console.error(`\n${cmd} ${args.join(" ")} failed`);
    process.exit(result.status ?? 1);
  }
}

// Keep local paths (a user name, a checkout folder) out of the committed binary.
const rustflags = [
  `--remap-path-prefix=${cargoHome}=/cargo`,
  `--remap-path-prefix=${relay}=/relay`,
  process.env.RUSTFLAGS || "",
].join(" ").trim();

run("cargo", ["build", "-p", "pm-relay-wasm", "--release", "--locked", "--target", "wasm32-unknown-unknown"], {
  cwd: relay,
  env: {
    ...process.env,
    RUSTFLAGS: rustflags,
    // Small and self-contained: this is what gets uploaded with the Worker.
    CARGO_PROFILE_RELEASE_OPT_LEVEL: "s",
    CARGO_PROFILE_RELEASE_LTO: "true",
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS: "1",
    CARGO_PROFILE_RELEASE_PANIC: "abort",
    CARGO_PROFILE_RELEASE_STRIP: "true",
  },
});

rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });
run(bindgen, [
  "--target", "web",
  "--out-dir", outDir,
  "--out-name", "pm_relay_wasm",
  join(targetDir, "wasm32-unknown-unknown", "release", "pm_relay_wasm.wasm"),
]);

// The generated loader falls back to `new URL(<file>, import.meta.url)` when called without a module;
// the Worker always passes the bundled module to initSync, so drop the fallback.
const glue = join(outDir, "pm_relay_wasm.js");
let text = readFileSync(glue, "utf8");
text = text.replace(/new URL\('pm_relay_wasm_bg\.wasm', import\.meta\.url\)/g, "undefined");
writeFileSync(glue, text);

const digest = createHash("sha256").update(readFileSync(join(outDir, "pm_relay_wasm_bg.wasm"))).digest("hex");
writeFileSync(
  join(outDir, "BUILD.txt"),
  `Built from relay/protocol and relay/worker/wasm by scripts/build-wasm.mjs.\n` +
    `wasm-bindgen 0.2.129, cargo build --release --target wasm32-unknown-unknown.\n` +
    `pm_relay_wasm_bg.wasm sha256 ${digest}\n`,
);
console.log(`\nwasm module written to ${outDir}\nsha256 ${digest}`);
