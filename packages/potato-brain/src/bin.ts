import { CONFIG } from "./config.ts";
import { makeAgentFactory } from "./agents.ts";
import { estimateTokens } from "./core/budget.ts";
import { makeProvider } from "./core/provider.ts";
import { runCli } from "./channels/cli.ts";
import { loadPersona, saveMood } from "./persona/persona.ts";

/**
 * Developer entry: a terminal chat with the brain, no app, no server.
 * Puppet Master starts `pm.ts` instead.
 */
const pack = () => (process.env.FISHD_PACK ?? "").trim();

function personaTokens(): number {
  return loadPersona(CONFIG.personaDir, pack()).reduce(
    (n, block) => n + estimateTokens(block.content),
    0,
  );
}

function runCheck(): void {
  try {
    console.log(`provider : ${makeProvider().name}`);
  } catch (err) {
    console.log(`provider : ! ${err instanceof Error ? err.message : String(err)}`);
  }
  console.log(`model    : ${process.env.FISHD_MODEL || "(default)"}`);
  const tokens = personaTokens();
  console.log(
    `persona  : ${tokens} tok / ${CONFIG.staticBudget}` +
      (tokens > CONFIG.staticBudget ? "  ! over the limit" : ""),
  );
}

async function main(): Promise<void> {
  if (process.argv.includes("--check")) {
    runCheck();
    return;
  }

  const tokens = personaTokens();
  if (tokens > CONFIG.staticBudget) {
    console.warn(
      `! persona is ${tokens} tokens, over the limit of ${CONFIG.staticBudget}. ` +
        `The first message will fail; shorten the persona files or raise FISHD_STATIC_BUDGET.`,
    );
  }

  const agent = makeAgentFactory({ config: CONFIG, pack })();
  await runCli(agent);
  saveMood(CONFIG.stateDir, agent.moodState);
}

main().catch((err: unknown) => {
  console.error(err instanceof Error ? err.message : err);
  process.exit(1);
});
