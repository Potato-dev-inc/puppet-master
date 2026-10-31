import type { Block } from "../types.ts";

export interface Budget {
  static: number;
  stable: number;
  chain: number;
  hot: number;
  tip: number;
}

export const DEFAULT_BUDGET: Budget = {
  static: 4000,
  stable: 500,
  chain: 8000,
  hot: 12000,
  tip: 1500,
};

/**
 * Rough token estimate. CJK is about one token per character, latin is
 * about one per 3.6 characters. Only used for budgeting and display;
 * real usage always comes back from the provider.
 */
export function estimateTokens(s: string): number {
  let cjk = 0;
  let other = 0;
  for (const ch of s) {
    const cp = ch.codePointAt(0) ?? 0;
    const isCjk =
      (cp >= 0x2e80 && cp <= 0x9fff) ||
      (cp >= 0xf900 && cp <= 0xfaff) ||
      (cp >= 0xff00 && cp <= 0xffef);
    if (isCjk) cjk++;
    else other++;
  }
  return Math.ceil(cjk * 0.95 + other / 3.6);
}

/**
 * Keep the head and tail of the checkpoint chain and drop from the middle.
 * The head holds the goal; the tail holds the recent state. Losing the
 * middle is cheaper than losing either end.
 */
export function fitChain(
  blocks: Block[],
  cap: number,
): { kept: Block[]; dropped: string[] } {
  const total = blocks.reduce((n, b) => n + estimateTokens(b.content), 0);
  if (total <= cap || blocks.length <= 2) return { kept: blocks, dropped: [] };

  const kept: Block[] = [];
  const dropped: string[] = [];

  // Fill from both ends, alternating, until the cap is reached.
  let lo = 0;
  let hi = blocks.length - 1;
  let used = 0;
  const picked = new Set<number>();

  while (lo <= hi) {
    for (const i of [lo, hi]) {
      const b = blocks[i];
      if (!b || picked.has(i)) continue;
      const t = estimateTokens(b.content);
      if (used + t <= cap) {
        picked.add(i);
        used += t;
      }
    }
    lo++;
    hi--;
  }

  for (let i = 0; i < blocks.length; i++) {
    const b = blocks[i];
    if (!b) continue;
    if (picked.has(i)) kept.push(b);
    else dropped.push(b.id);
  }
  return { kept, dropped };
}
