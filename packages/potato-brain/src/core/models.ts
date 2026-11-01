import { readAuthKey } from "./opencode-auth.ts";

export interface ModelSource {
  provider: "opencode" | "deepseek";
  modelsUrl: string;
  apiKey: string;
}

function goKey(): string {
  return (
    (process.env.OPENCODE_API_KEY ?? "").trim() ||
    readAuthKey("opencode-go") ||
    readAuthKey("opencode") ||
    ""
  );
}

/**
 * Which catalog to list. Mirrors makeProvider's precedence: an explicit
 * FISHD_PROVIDER wins, otherwise a DeepSeek key beats an incidental Go key.
 */
export function resolveModelSource(provider?: string): ModelSource | null {
  const choice = (provider ?? process.env.FISHD_PROVIDER ?? "").trim().toLowerCase();
  const deepseekKey = (process.env.DEEPSEEK_API_KEY ?? "").trim();

  if (choice === "deepseek" || (choice === "" && deepseekKey !== "")) {
    if (deepseekKey === "") return null;
    return { provider: "deepseek", modelsUrl: "https://api.deepseek.com/models", apiKey: deepseekKey };
  }

  if (choice === "opencode" || choice === "") {
    const key = goKey();
    if (key === "") return null;
    const base = (process.env.OPENCODE_BASE_URL ?? "https://opencode.ai/inference/go/openai/v1").replace(
      /\/+$/,
      "",
    );
    // .../go/openai/v1 -> .../go/v1/models
    const modelsUrl = `${base.replace(/\/openai\/v1$/, "/v1")}/models`;
    return { provider: "opencode", modelsUrl, apiKey: key };
  }

  return null;
}

export async function listModels(source: ModelSource, signal?: AbortSignal): Promise<string[]> {
  const res = await fetch(source.modelsUrl, {
    headers: {
      Authorization: `Bearer ${source.apiKey}`,
      "User-Agent": "fishd/0.1",
    },
    signal,
  });

  if (!res.ok) throw new Error(`${source.provider} models ${res.status}`);

  const data = (await res.json()) as { data?: Array<{ id?: unknown }> };
  const ids = (data.data ?? [])
    .map((m) => m.id)
    .filter((id): id is string => typeof id === "string" && id !== "");

  return ids.sort((a, b) => a.localeCompare(b));
}

/**
 * Models that can look at a picture. A tool result may carry a screenshot only for these; for any other
 * model (DeepSeek, today's default, has no vision) the sandbox tools answer in words. The match is by
 * the start of the model name, lower case, longest first; a name that is not listed cannot see (the safe
 * answer: words always work, a picture sent to a model that cannot read it is a failed request).
 */
const VISION_PREFIXES: ReadonlyArray<string> = [
  "gpt-4o",
  "gpt-4.1",
  "gpt-4.5",
  "gpt-5",
  "o3",
  "o4",
  "claude-",
  "gemini-",
  "qwen-vl",
  "qwen2-vl",
  "qwen2.5-vl",
  "qwen3-vl",
  "kimi-k2.5",
  "kimi-vl",
  "glm-4v",
  "glm-4.5v",
  "glm-4.6v",
  "llama-4",
  "pixtral",
  "mistral-small-3",
  "grok-4",
];

/** Names that start like a seeing model but are not one. Listed first, they win. */
const TEXT_ONLY_PREFIXES: ReadonlyArray<string> = ["deepseek"];

/**
 * Can this model read a screenshot? `FISHD_VISION=on|off` overrides the table (for a model that is
 * not listed yet, or to switch pictures off).
 */
export function modelSeesImages(model: string, override: string | undefined = process.env.FISHD_VISION): boolean {
  const forced = (override ?? "").trim().toLowerCase();
  if (forced === "on" || forced === "1" || forced === "true") return true;
  if (forced === "off" || forced === "0" || forced === "false") return false;
  const name = model.trim().toLowerCase().replace(/^[^/]*\//, "");
  if (name === "") return false;
  if (TEXT_ONLY_PREFIXES.some((p) => name.startsWith(p))) return false;
  return VISION_PREFIXES.some((p) => name.startsWith(p));
}
