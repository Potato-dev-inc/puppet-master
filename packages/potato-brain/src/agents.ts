import { join } from "node:path";
import type { Config } from "./config.ts";
import { CacheLog } from "./core/cache.ts";
import { Agent } from "./core/loop.ts";
import type { ExtraTool } from "./core/loop.ts";
import { makeProvider } from "./core/provider.ts";
import { VaultMemory } from "./memory/vaultMemory.ts";
import { resolvePersonaDir } from "./persona/packs.ts";
import {
  appendMoodLog,
  appendSelfLog,
  loadPersona,
  moodStore,
  selfStore,
} from "./persona/persona.ts";
import type { Provider } from "./types.ts";

export interface FactoryOptions {
  config: Config;
  /** the pack in use right now; read on every turn, so a switch needs no restart */
  pack: () => string;
  /** defaults to the provider the environment asks for */
  provider?: () => Provider;
  /**
   * Where she keeps notes about herself. Default: next to the persona files,
   * which is right for a person's own checkout and wrong for an installed
   * app, so the Potato entry point passes a folder under its state dir.
   */
  notesDir?: string;
  /** offer the share tool (a folder of gifs). Off under Puppet Master. */
  share?: boolean;
  now?: () => Date;
}

export type AgentFactory = (where?: string, extraTools?: ExtraTool[]) => Agent;

/**
 * One shared vault, one shared mood, one shared cache log, but a separate
 * conversation window per room, so a private message never leaks into a
 * public room's verbatim history. Call the returned function once per room.
 */
export function makeAgentFactory(opts: FactoryOptions): AgentFactory {
  const { config } = opts;
  const loader = () => loadPersona(config.personaDir, opts.pack());
  const memory = new VaultMemory(config.vaultRoot);
  const cache = new CacheLog();
  const store = moodStore(config.stateDir);
  const self = selfStore(config.stateDir);
  const notesDir =
    opts.notesDir ?? resolvePersonaDir(config.personaDir, opts.pack());

  return (where, extraTools) =>
    new Agent({
      provider: (opts.provider ?? makeProvider)(),
      persona: loader(),
      personaLoader: loader,
      mood: store.load(),
      moodStore: store,
      selfStore: self,
      onSelfChange: (_next, log) => appendSelfLog(config.stateDir, log.join("; ")),
      memory,
      cache,
      budget: {
        static: config.staticBudget,
        stable: config.stableBudget,
        chain: 8000,
        hot: 12000,
        tip: 1500,
      },
      hotTurns: config.hotTurns,
      maxTokens: config.maxTokens,
      temperature: config.temperature,
      voice: config.voice,
      where,
      notesDir,
      sessionsDir: config.vaultRoot,
      ...(opts.share === false ? {} : { shareDir: config.shareDir }),
      compactEvery: config.hotTurns,
      onMoodChange: (_next, log) => appendMoodLog(config.stateDir, log),
      ...(opts.now ? { now: opts.now } : {}),
      ...(extraTools ? { extraTools } : {}),
    });
}

export function notesDirUnder(stateDir: string): string {
  return join(stateDir, "notes");
}
