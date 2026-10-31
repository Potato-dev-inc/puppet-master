import { envNumber, loadEnv } from "./env.ts";

// Under Puppet Master (PM_POTATO_URL is set) the environment is the whole
// configuration: it was built by the app, and a stray .env in the working
// folder must not add to it.
if ((process.env.PM_POTATO_URL ?? "").trim() === "") loadEnv();

export interface Config {
  model: string;
  temperature: number;
  maxTokens: number;
  vaultRoot: string;
  personaDir: string;
  /** gifs and short videos she is allowed to send */
  shareDir: string;
  staticBudget: number;
  stableBudget: number;
  stateDir: string;
  /** how many trailing turns to keep verbatim */
  hotTurns: number;
  voice: { enabled: boolean; probability: number; maxChars: number };
}

const VAULT = (process.env.FISHD_VAULT ?? "").trim() || "vault";

export const CONFIG: Config = {
  model: process.env.FISHD_MODEL ?? "deepseek-chat",
  temperature: envNumber("FISHD_TEMP", 1.0, 0),
  maxTokens: envNumber("FISHD_MAX_TOKENS", 600, 1),
  vaultRoot: VAULT,
  personaDir: (process.env.FISHD_PERSONA_DIR ?? "").trim() || `${VAULT}/persona`,
  // `??` would keep the empty string that .env.example ships
  shareDir: (process.env.FISHD_SHARE_DIR ?? "").trim() || `${VAULT}/share`,
  staticBudget: envNumber("FISHD_STATIC_BUDGET", 4000, 500),
  stableBudget: envNumber("FISHD_STABLE_BUDGET", 900, 100),
  stateDir: (process.env.FISHD_STATE_DIR ?? "").trim() || `${VAULT}/state`,
  hotTurns: envNumber("FISHD_HOT_TURNS", 24, 1),
  voice: {
    enabled: process.env.FISHD_VOICE !== "0",
    probability: 0.5,
    maxChars: 60,
  },
};
