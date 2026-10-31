export type Role = "system" | "user" | "assistant" | "tool";

export interface ToolCall {
  id: string;
  name: string;
  arguments: string;
}

export interface ToolSpec {
  name: string;
  description: string;
  parameters: Record<string, unknown>;
}

export interface Msg {
  role: Role;
  content: string;
  /** assistant only */
  toolCalls?: ToolCall[];
  /**
   * assistant only, thinking models.
   * DeepSeek requires this to be echoed back verbatim when `tools` is present,
   * otherwise every tool round fails with 400.
   */
  reasoning?: string;
  /** tool result only */
  toolCallId?: string;
}

/**
 * Prompt block volatility, in ascending order.
 *  0 static   persona core + rules + tool schemas   (never changes)
 *  1 stable   long-term profile                     (changes ~weekly)
 *  2 append   checkpoint chain                      (append-only)
 *  3 hot      recent turns                          (every turn)
 *  4 tip      mood, clock, retrieval                (last message only)
 *
 * The KV cache only survives if bytes 0..1 never change. Everything
 * above the prefix is allowed to churn; everything below is not.
 */
export type Volatility = 0 | 1 | 2 | 3 | 4;

export interface Block {
  id: string;
  volatility: Volatility;
  content: string;
}

export interface Prompt {
  messages: Msg[];
  blocks: Block[];
  /** hash of the immutable prefix (volatility 0 and 1) */
  prefixHash: string;
  /** hash of the whole rendered prompt, for diagnostics only */
  promptHash: string;
  tokens: number;
  dropped: string[];
}

export interface Usage {
  promptTokens: number;
  completionTokens: number;
  cacheHitTokens: number;
  cacheMissTokens: number;
}

export interface CacheStat {
  requests: number;
  hitTokens: number;
  missTokens: number;
  promptTokens: number;
  completionTokens: number;
  costUsd: number;
  naiveCostUsd: number;
}

export interface ChatRequest {
  messages: Msg[];
  model?: string;
  temperature?: number;
  maxTokens?: number;
  signal?: AbortSignal;
  tools?: ToolSpec[];
  /** harness-only metadata; providers may ignore it */
  meta?: { prefixHash?: string; session?: string };
}

export interface ChatResult {
  text: string;
  usage: Usage;
  model: string;
  toolCalls?: ToolCall[];
  /** thinking models only: the chain of thought, if the provider returned one */
  reasoning?: string;
}

export interface Provider {
  readonly name: string;
  chat(req: ChatRequest): Promise<ChatResult>;
}
