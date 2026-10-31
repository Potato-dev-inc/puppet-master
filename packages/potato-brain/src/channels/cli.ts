import { createInterface } from "node:readline/promises";
import { stdin, stdout } from "node:process";
import type { Agent } from "../core/loop.ts";

const DIM = "\u001b[2m";
const RESET = "\u001b[0m";
const FISH = "\u001b[36m";

type Outcome = "continue" | "quit";

function show(segments: string[], footer: string): void {
  for (const s of segments) stdout.write(`${FISH}🐟${RESET} ${s}\n`);
  stdout.write(`${DIM}   ${footer}${RESET}\n`);
}

async function handle(agent: Agent, input: string): Promise<Outcome> {
  if (input === "") return "continue";
  if (input === "/quit" || input === "/exit") return "quit";

  if (input === "/help") {
    stdout.write(`${DIM}/mood  心情原始狀態\n/stats 快取與花費\n/quit  離開${RESET}\n`);
    return "continue";
  }
  if (input === "/mood") {
    stdout.write(`${DIM}${JSON.stringify(agent.moodState)}${RESET}\n`);
    return "continue";
  }
  if (input === "/stats") {
    const s = agent.cacheStat;
    const total = s.hitTokens + s.missTokens;
    const rate = total === 0 ? 0 : s.hitTokens / total;
    stdout.write(
      `${DIM}turns ${s.requests} · prefix ${agent.personalityHash} · ` +
        `hit ${(rate * 100).toFixed(0)}% · ` +
        `cost $${s.costUsd.toFixed(5)} (naive $${s.naiveCostUsd.toFixed(5)})${RESET}\n`,
    );
    return "continue";
  }

  try {
    const r = await agent.turn(input);
    if (process.env.FISHD_DEBUG === "1") {
      const last = r.prompt.messages[r.prompt.messages.length - 1];
      stdout.write(`${DIM}   tip: ${JSON.stringify(last?.content)}${RESET}\n`);
    }
    show(r.segments, r.footer);
    if (r.nudgeLog) stdout.write(`${DIM}   mood ${r.nudgeLog}${RESET}\n`);
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    stdout.write(`${DIM}   ! ${msg}${RESET}\n`);
  }
  return "continue";
}

/**
 * Piped stdin is read up front and replayed line by line. readline/promises
 * drops buffered lines when a closed pipe lands between questions, which
 * makes scripted runs silently stop after one turn.
 */
async function runPiped(agent: Agent): Promise<void> {
  const chunks: Buffer[] = [];
  for await (const chunk of stdin) chunks.push(Buffer.from(chunk as Uint8Array));
  const lines = Buffer.concat(chunks)
    .toString("utf8")
    .split(/\r?\n/)
    .map((s) => s.trim())
    .filter(Boolean);

  for (const line of lines) {
    const out = await handle(agent, line);
    if (out === "quit") break;
  }
}

async function runInteractive(agent: Agent): Promise<void> {
  const rl = createInterface({ input: stdin, output: stdout });
  process.on("SIGINT", () => {
    rl.close();
    process.exit(0);
  });

  while (true) {
    let line: string;
    try {
      line = await rl.question(`${FISH}你${RESET} `);
    } catch {
      break;
    }
    const out = await handle(agent, line.trim());
    if (out === "quit") break;
  }
  rl.close();
}

export async function runCli(agent: Agent): Promise<void> {
  stdout.write(`${DIM}fishd · ${agent.providerName} 模式。 /help 看指令。${RESET}\n`);
  if (!stdin.isTTY) await runPiped(agent);
  else await runInteractive(agent);
}
