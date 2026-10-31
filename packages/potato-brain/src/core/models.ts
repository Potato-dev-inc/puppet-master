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
