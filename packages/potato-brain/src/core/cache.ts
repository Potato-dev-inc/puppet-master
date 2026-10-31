import type { CacheStat, Usage } from "../types.ts";

export interface Rates {
  /** USD per 1M tokens */
  cacheHit: number;
  cacheMiss: number;
  output: number;
}

/** DeepSeek V3.2 list prices. Verify before trusting any number below. */
export const DEEPSEEK_RATES: Rates = {
  cacheHit: 0.028,
  cacheMiss: 0.28,
  output: 0.42,
};

export function emptyStat(): CacheStat {
  return {
    requests: 0,
    hitTokens: 0,
    missTokens: 0,
    promptTokens: 0,
    completionTokens: 0,
    costUsd: 0,
    naiveCostUsd: 0,
  };
}

export class CacheLog {
  stat: CacheStat;
  rates: Rates;

  constructor(rates: Rates = DEEPSEEK_RATES) {
    this.stat = emptyStat();
    this.rates = rates;
  }

  record(u: Usage): CacheStat {
    const s = this.stat;
    s.requests += 1;
    s.hitTokens += u.cacheHitTokens;
    s.missTokens += u.cacheMissTokens;
    s.promptTokens += u.promptTokens;
    s.completionTokens += u.completionTokens;

    s.costUsd +=
      (u.cacheHitTokens * this.rates.cacheHit) / 1e6 +
      (u.cacheMissTokens * this.rates.cacheMiss) / 1e6 +
      (u.completionTokens * this.rates.output) / 1e6;

    // What the same traffic would have cost with no prefix caching at all.
    s.naiveCostUsd +=
      (u.promptTokens * this.rates.cacheMiss) / 1e6 +
      (u.completionTokens * this.rates.output) / 1e6;

    return s;
  }

  get hitRate(): number {
    const total = this.stat.hitTokens + this.stat.missTokens;
    return total === 0 ? 0 : this.stat.hitTokens / total;
  }

  get savedUsd(): number {
    return Math.max(0, this.stat.naiveCostUsd - this.stat.costUsd);
  }

  /** One-line footer for the CLI. */
  formatLast(u: Usage): string {
    const total = u.cacheHitTokens + u.cacheMissTokens;
    const rate = total === 0 ? 0 : u.cacheHitTokens / total;
    const all = this.stat;
    return (
      `prompt ${u.promptTokens.toLocaleString()} · ` +
      `hit ${(rate * 100).toFixed(0)}% · ` +
      `out ${u.completionTokens} · ` +
      `turn $${((u.cacheHitTokens * this.rates.cacheHit + u.cacheMissTokens * this.rates.cacheMiss + u.completionTokens * this.rates.output) / 1e6).toFixed(5)} · ` +
      `session ${(all.requests)} turns · ` +
      `hit ${(this.hitRate * 100).toFixed(0)}% · saved $${this.savedUsd.toFixed(4)}`
    );
  }
}
