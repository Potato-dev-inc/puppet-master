import type { Block, Msg, Prompt } from "../types.ts";
import { hashBlocks, sha } from "./hash.ts";
import { DEFAULT_BUDGET, estimateTokens, fitChain } from "./budget.ts";
import type { Budget } from "./budget.ts";
import { CodedError } from "./errors.ts";

export interface AssembleInput {
  /** volatility 0 — persona core, rules, tool schemas */
  static: Block[];
  /** volatility 1 — long-term profile */
  stable: Block[];
  /** volatility 2 — checkpoint chain, append-only */
  chain: Block[];
  /** volatility 3 — recent turns */
  hot: Msg[];
  /** volatility 4 — appended to the newest user message */
  tip: Block | null;
}

export interface AssembleOptions {
  budget?: Budget;
}

const SEP = "\n\n";

function joinBlocks(blocks: Block[]): string {
  return blocks.map((b) => b.content.trim()).join(SEP);
}

/**
 * Render state -> messages. Pure: same input, same bytes, always.
 * Any nondeterminism here is a cache miss, so there is none: the caller
 * passes in the clock, the mood, and the retrieval result.
 */
export function assemble(input: AssembleInput, opts: AssembleOptions = {}): Prompt {
  const budget = opts.budget ?? DEFAULT_BUDGET;
  const dropped: string[] = [];

  const staticTokens = input.static.reduce((n, b) => n + estimateTokens(b.content), 0);
  const stableTokens = input.stable.reduce((n, b) => n + estimateTokens(b.content), 0);
  if (staticTokens > budget.static) {
    // The prefix is not trimmable. Shout instead of silently rotting the cache.
    throw new CodedError(
      "PERSONA_TOO_BIG",
      `static prefix over budget: ${staticTokens} > ${budget.static} tokens. ` +
        `Raise FISHD_STATIC_BUDGET, or trim persona/core.md + rules.md + pasted.md.`,
    );
  }
  if (stableTokens > budget.stable) {
    throw new CodedError(
      "INTERNAL",
      `stable block over budget: ${stableTokens} > ${budget.stable} tokens. ` +
        `Move detail into memory nodes and leave pointers here.`,
    );
  }

  const chain = fitChain(input.chain, budget.chain);
  dropped.push(...chain.dropped);

  const hot = trimHot(input.hot, budget.hot, dropped);

  const systemBlocks = [...input.static, ...input.stable, ...chain.kept];
  const messages: Msg[] = [{ role: "system", content: joinBlocks(systemBlocks) }, ...hot];

  if (input.tip) {
    const tipText = input.tip.content.trim();
    const last = messages[messages.length - 1];
    if (last && last.role === "user") {
      last.content = `${last.content}${SEP}${tipText}`;
    } else {
      messages.push({ role: "user", content: tipText });
    }
  }

  const tokens = messages.reduce((n, m) => n + estimateTokens(m.content), 0);

  return {
    messages,
    blocks: [...input.static, ...input.stable, ...chain.kept],
    prefixHash: hashBlocks([...input.static, ...input.stable]),
    promptHash: sha(JSON.stringify(messages)),
    tokens,
    dropped,
  };
}

function trimHot(hot: Msg[], cap: number, dropped: string[]): Msg[] {
  const out = hot.slice();
  let total = out.reduce((n, m) => n + estimateTokens(m.content), 0);
  while (total > cap && out.length > 1) {
    const removed = out.shift();
    if (!removed) break;
    total -= estimateTokens(removed.content);
    dropped.push(`hot:${out.length}`);
  }
  return out;
}

/**
 * Guard against the single bug that costs 10x: a byte changed in the prefix.
 * Call this in development (and in tests) on every turn.
 */
export function assertPrefix(p: Prompt, expected: string): void {
  if (p.prefixHash !== expected) {
    throw new Error(
      `prefix changed: ${expected} -> ${p.prefixHash}. ` +
        `Something volatile leaked into volatility 0/1.`,
    );
  }
}
