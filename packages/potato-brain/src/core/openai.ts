import type { ChatRequest, ChatResult, Msg, Provider, ToolCall, Usage } from "../types.ts";
import { estimateTokens } from "./budget.ts";
import { CodedError, codeForStatus } from "./errors.ts";

export interface OpenAIOptions {
  apiKey: string;
  /** e.g. https://api.deepseek.com or https://opencode.ai/inference/go/openai/v1 */
  baseUrl: string;
  model: string;
  name?: string;
  headers?: Record<string, string>;
  /**
   * "auto" sends nothing and lets the server decide (DeepSeek enables thinking
   * by default). Only providers known to accept the field are given a value.
   */
  thinking?: "auto" | "enabled" | "disabled";
  /** low | high | max — DeepSeek maps medium/xhigh/ultra itself */
  reasoningEffort?: string;
}

interface UsageShape {
  prompt_tokens?: number;
  completion_tokens?: number;
  prompt_cache_hit_tokens?: number;
  prompt_cache_miss_tokens?: number;
  prompt_tokens_details?: { cached_tokens?: number };
}

function promptTokensOf(messages: Msg[]): number {
  return messages.reduce((n, m) => n + estimateTokens(m.content), 0);
}

function toWire(msg: Msg): Record<string, unknown> {
  const wire: Record<string, unknown> = { role: msg.role, content: msg.content };
  if (msg.toolCalls && msg.toolCalls.length > 0) {
    wire["tool_calls"] = msg.toolCalls.map((call) => ({
      id: call.id,
      type: "function",
      function: { name: call.name, arguments: call.arguments },
    }));
  }
  if (msg.toolCallId) wire["tool_call_id"] = msg.toolCallId;
  // DeepSeek: with `tools` in the request, prior reasoning must come back
  // verbatim or the API rejects the whole round.
  if (msg.reasoning && msg.role === "assistant") wire["reasoning_content"] = msg.reasoning;
  return wire;
}

interface WireToolCall {
  id?: unknown;
  function?: { name?: unknown; arguments?: unknown };
}

function parseToolCalls(raw: unknown): ToolCall[] {
  if (!Array.isArray(raw)) return [];
  const calls: ToolCall[] = [];
  for (const item of raw as WireToolCall[]) {
    const name = typeof item?.function?.name === "string" ? item.function.name : "";
    if (name === "") continue;
    calls.push({
      id: typeof item.id === "string" ? item.id : `call-${calls.length}`,
      name,
      arguments:
        typeof item.function?.arguments === "string" ? item.function.arguments : "{}",
    });
  }
  return calls;
}

/**
 * Any OpenAI-compatible chat completions endpoint: DeepSeek, OpenCode Go,
 * OpenRouter, vLLM, llama.cpp. One implementation, no SDK.
 */
export class OpenAICompatProvider implements Provider {
  readonly name: string;
  private apiKey: string;
  private baseUrl: string;
  private defaultModel: string;
  private extraHeaders: Record<string, string>;
  private thinking: "auto" | "enabled" | "disabled";
  private reasoningEffort: string;

  constructor(opts: OpenAIOptions) {
    this.name = opts.name ?? "openai-compatible";
    this.apiKey = opts.apiKey;
    this.baseUrl = opts.baseUrl.replace(/\/+$/, "");
    this.defaultModel = opts.model;
    this.extraHeaders = opts.headers ?? {};
    this.thinking = opts.thinking ?? "auto";
    this.reasoningEffort = (opts.reasoningEffort ?? "").trim().toLowerCase();
  }

  async chat(req: ChatRequest): Promise<ChatResult> {
    const model = req.model ?? this.defaultModel;

    let res: Response;
    try {
      res = await fetch(`${this.baseUrl}/chat/completions`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${this.apiKey}`,
          "User-Agent": "fishd/0.1",
          ...this.extraHeaders,
        },
        body: JSON.stringify({
          model,
          messages: req.messages.map(toWire),
          ...(req.tools && req.tools.length > 0
            ? {
                tools: req.tools.map((tool) => ({
                  type: "function",
                  function: {
                    name: tool.name,
                    description: tool.description,
                    parameters: tool.parameters,
                  },
                })),
              }
            : {}),
          temperature: req.temperature ?? 1.0,
          max_tokens: req.maxTokens ?? 400,
          ...(this.thinking === "auto" ? {} : { thinking: { type: this.thinking } }),
          ...(this.reasoningEffort === "" || this.reasoningEffort === "auto"
            ? {}
            : { reasoning_effort: this.reasoningEffort }),
          stream: false,
        }),
        signal: req.signal,
      });
    } catch {
      // the cause can carry the url or headers; the code is all that leaves
      throw new CodedError("NETWORK", `${this.name}: network error`);
    }

    if (!res.ok) {
      // The body is never read: it can echo the request or the account.
      void res.body?.cancel().catch(() => {});
      throw new CodedError(codeForStatus(res.status), `${this.name} ${res.status}`, res.status);
    }

    let data: {
      choices?: Array<{
        message?: { content?: string; reasoning_content?: string; tool_calls?: unknown };
        finish_reason?: string;
      }>;
      usage?: UsageShape;
    };
    try {
      data = (await res.json()) as typeof data;
    } catch {
      throw new CodedError("PROVIDER_DOWN", `${this.name}: unreadable reply`);
    }

    const choice = data.choices?.[0];
    const text = choice?.message?.content ?? "";
    const toolCalls = parseToolCalls(choice?.message?.tool_calls);
    const reasoning = choice?.message?.reasoning_content ?? "";

    if (text.trim() === "" && toolCalls.length === 0) {
      const finish = choice?.finish_reason ?? "unknown";
      throw new CodedError(
        "PROVIDER_DOWN",
        `${this.name}/${model}: empty reply (finish=${finish}). ` +
          `Reasoning models spend the budget thinking before answering; raise max_tokens or pick a non-reasoning model.`,
      );
    }
    const u = data.usage ?? {};
    const promptTokens = u.prompt_tokens ?? promptTokensOf(req.messages);
    const reported = u.prompt_cache_hit_tokens ?? u.prompt_tokens_details?.cached_tokens ?? 0;
    const hit = Math.max(0, Math.min(reported, promptTokens));
    const miss = u.prompt_cache_miss_tokens ?? Math.max(0, promptTokens - hit);

    const usage: Usage = {
      promptTokens,
      completionTokens: u.completion_tokens ?? estimateTokens(text),
      cacheHitTokens: hit,
      cacheMissTokens: miss,
    };

    return {
      text,
      usage,
      model,
      toolCalls: toolCalls.length > 0 ? toolCalls : undefined,
      reasoning: reasoning === "" ? undefined : reasoning,
    };
  }
}
