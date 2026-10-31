import { randomUUID } from "node:crypto";
import type { ChatRequest, ChatResult, Msg, Provider, Usage } from "../types.ts";
import { estimateTokens } from "./budget.ts";
import { OpenAICompatProvider } from "./openai.ts";
import { readAuthKey } from "./opencode-auth.ts";

export { OpenAICompatProvider } from "./openai.ts";

function countPrompt(messages: Msg[]): number {
  return messages.reduce((n, m) => n + estimateTokens(m.content), 0);
}

/**
 * Offline provider. It simulates prefix caching the way a real one behaves:
 * the hit equals the length of the previous prompt when the prefix hash is
 * unchanged, and zero when it is not.
 */
export class StubProvider implements Provider {
  readonly name = "stub";
  private lastPromptTokens = 0;
  private lastPrefix = "";
  private turn = 0;
  private replies: string[];

  constructor(replies?: string[]) {
    this.replies = replies ?? [
      "唔……我在喔。*尾巴擺了一下*",
      "欸嘿，這個我知道啦 (๑•̀ㅂ•́)و✧",
      "哼，才、才不是特意回答你的 (〃▽〃)",
      "好喔，不過等一下要吃飯，白飯 (๑´ڡ`๑)",
    ];
  }

  async chat(req: ChatRequest): Promise<ChatResult> {
    const promptTokens = countPrompt(req.messages);
    const prefix = req.meta?.prefixHash ?? "";
    const cached = prefix !== "" && prefix === this.lastPrefix;

    const hit = cached ? Math.min(this.lastPromptTokens, promptTokens) : 0;
    const usage: Usage = {
      promptTokens,
      completionTokens: 24,
      cacheHitTokens: hit,
      cacheMissTokens: promptTokens - hit,
    };

    this.lastPromptTokens = promptTokens;
    this.lastPrefix = prefix;

    const text = this.replies[this.turn % this.replies.length] ?? "……";
    this.turn++;
    return { text, usage, model: "stub" };
  }
}

const GO_BASE = "https://opencode.ai/inference/go/openai/v1";

function modelFor(fallback: string): string {
  const model = (process.env.FISHD_MODEL ?? "").trim();
  if (model === "") return fallback;
  // the old default, still sitting in some .env files
  if (model === "deepseek-chat") return "deepseek-v4.1-flash";
  return model;
}

type Mode = "opencode" | "deepseek" | "stub";

function chooseMode(choice: string, hasGo: boolean, hasDeepSeek: boolean): Mode {
  if (choice === "opencode" || choice === "deepseek" || choice === "stub") return choice;
  // An explicitly set key beats an incidental one found in opencode's auth file,
  // otherwise every opencode user would silently spend their Go allowance.
  if (hasDeepSeek) return "deepseek";
  if (hasGo) return "opencode";
  return "stub";
}

function thinkingFor(): "auto" | "enabled" | "disabled" {
  const raw = (process.env.FISHD_REASONING ?? "").trim().toLowerCase();
  if (raw === "off" || raw === "0" || raw === "disabled" || raw === "false") return "disabled";
  if (raw === "on" || raw === "1" || raw === "enabled" || raw === "true") return "enabled";
  return "auto";
}

function effortFor(): string {
  const raw = (process.env.FISHD_REASONING_EFFORT ?? "").trim().toLowerCase();
  return raw === "" ? "" : raw;
}

export function makeProvider(): Provider {
  const choice = (process.env.FISHD_PROVIDER ?? "").trim().toLowerCase();
  // The offline stub touches nothing: no key, no login file, no network.
  if (choice === "stub") return new StubProvider();

  // An empty value in .env is "not set", not a key.
  const deepseekKey = (process.env.DEEPSEEK_API_KEY ?? "").trim();
  let goKey = (process.env.OPENCODE_API_KEY ?? "").trim();
  // The login file is read only when the opencode route can actually be taken.
  const mayUseGo = choice === "opencode" || (choice !== "deepseek" && deepseekKey === "");
  if (goKey === "" && mayUseGo) {
    goKey = readAuthKey("opencode-go") || readAuthKey("opencode") || "";
  }

  const mode = chooseMode(choice, goKey !== "", deepseekKey !== "");

  if (mode === "opencode") {
    if (!goKey) {
      throw new Error("FISHD_PROVIDER=opencode but no key was found (OPENCODE_API_KEY or the saved login file)");
    }
    return new OpenAICompatProvider({
      name: "opencode-go",
      apiKey: goKey,
      baseUrl: process.env.OPENCODE_BASE_URL ?? GO_BASE,
      model: modelFor("deepseek-v4.1-flash"),
      headers: {
        "x-opencode-session": randomUUID(),
        "User-Agent": "fishd/0.1",
      },
    });
  }

  if (mode === "deepseek") {
    const explicit = (process.env.FISHD_MODEL ?? "").trim();
    return new OpenAICompatProvider({
      name: "deepseek",
      apiKey: deepseekKey,
      baseUrl: "https://api.deepseek.com",
      model: explicit === "" || explicit === "deepseek-chat" ? "deepseek-flash" : explicit,
      thinking: thinkingFor(),
      reasoningEffort: effortFor(),
    });
  }

  return new StubProvider();
}
