/**
 * The entry point Puppet Master starts: `node brain.mjs` in the bot's own
 * folder. The environment is the whole configuration (see config.ts), and the
 * only way in or out is channel protocol v1 over the app's local bridge.
 *
 * Exit codes: 0 after a shutdown message, 2 when started without its
 * environment, 3 when the app is gone or refuses this brain.
 */
import { PotatoChannel } from "./channels/potato.ts";
import { BRAIN_VERSION } from "./version.ts";

const MISSING =
  "potato-brain: PM_POTATO_URL and PM_POTATO_TOKEN are not set. " +
  "This program is started by Puppet Master and does not run by itself.";

function env(name: string): string {
  return (process.env[name] ?? "").trim();
}

function isLoopbackUrl(raw: string): boolean {
  try {
    const url = new URL(raw);
    const host = url.hostname.replace(/^\[|\]$/g, "");
    return (
      url.protocol === "http:" &&
      (host === "127.0.0.1" || host === "localhost" || host === "::1")
    );
  } catch {
    return false;
  }
}

async function main(): Promise<void> {
  const url = env("PM_POTATO_URL");
  const token = env("PM_POTATO_TOKEN");
  if (url === "" || token === "") {
    console.error(MISSING);
    process.exit(2);
  }
  // The token goes to this address and nowhere else.
  if (!isLoopbackUrl(url)) {
    console.error("potato-brain: PM_POTATO_URL must be an http address on this computer.");
    process.exit(2);
  }

  // Loaded only now: config.ts reads the environment when it is first imported.
  const { CONFIG } = await import("./config.ts");
  const { makeAgentFactory, notesDirUnder } = await import("./agents.ts");
  const { makeProvider } = await import("./core/provider.ts");
  const { listPacks } = await import("./persona/packs.ts");

  const packs = listPacks(CONFIG.personaDir);
  let pack = env("FISHD_PACK");
  if (!packs.includes(pack)) pack = packs[0] ?? "";

  let provider = "none";
  let providerReady = false;
  try {
    const made = makeProvider();
    provider = made.name;
    // With no key the choice falls back to the offline stub. That is fine when
    // someone asked for it, and a missing key when they did not.
    providerReady = made.name !== "stub" || env("FISHD_PROVIDER").toLowerCase() === "stub";
    if (!providerReady) provider = "none";
  } catch {
    provider = "none";
  }

  const agentFor = makeAgentFactory({
    config: CONFIG,
    pack: () => pack,
    notesDir: notesDirUnder(CONFIG.stateDir),
    share: false,
  });

  const channel = new PotatoChannel({
    url,
    token,
    info: { version: BRAIN_VERSION, provider, packs, surfaces: ["app"] },
    providerReady,
    agentFor: (conversationId, extraTools) => agentFor(conversationId, extraTools),
    setPack: (next) => {
      pack = next;
    },
    onExit: (code) => process.exit(code),
    log: (line) => console.error(`potato-brain: ${line}`),
  });

  const leave = (): void => {
    void channel.stop().finally(() => process.exit(0));
  };
  process.on("SIGINT", leave);
  process.on("SIGTERM", leave);
  process.on("unhandledRejection", () => console.error("potato-brain: an unhandled error was ignored"));

  console.error(`potato-brain ${BRAIN_VERSION}: ready`);
  await channel.start();
}

main().catch(() => {
  console.error("potato-brain: could not start");
  process.exit(1);
});
