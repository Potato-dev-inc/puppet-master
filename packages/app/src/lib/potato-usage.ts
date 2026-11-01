// Potato Bot's Usage page: the numbers of the last 30 days (`potato_usage` in commands/potato_usage.rs, built by
// src-tauri/src/potato/usage.rs). Read only. Money comes from Rust in nano-dollars (a billionth of a dollar); this file
// turns it into the words the page shows. `fakeUsageTransport` is the in-memory twin for tests.
import { invoke } from '@tauri-apps/api/core';
import { dollars, plainError } from './potato';

/** One nano-dollar is a billionth of a dollar; one cent is 10,000,000 of them. */
export const NANO_PER_CENT = 10_000_000;

export interface UsageTotals {
  /** Brain turns the model reported usage for. */
  messages: number;
  /** Prompt plus completion tokens. */
  tokens: number;
  costNano: number;
  sandboxActions: number;
}

export interface UsageDay extends UsageTotals {
  /** `YYYY-MM-DD`, in the person's local time. */
  date: string;
}

export interface UsageBot {
  id: string;
  name: string;
  total: UsageTotals;
  today: UsageTotals;
  /** Oldest first. */
  days: UsageDay[];
}

export interface UsageReport {
  today: string;
  /** One bot's daily limit, in cents. */
  limitCents: number;
  /** One bot's daily cap of sandbox actions. */
  sandboxActionsPerDay: number;
  total: UsageTotals;
  todayTotal: UsageTotals;
  /** All bots, one per day, oldest first. */
  days: UsageDay[];
  bots: UsageBot[];
}

export interface PotatoUsageTransport {
  usage(): Promise<UsageReport>;
}

export const tauriUsageTransport: PotatoUsageTransport = {
  usage: () => invoke<UsageReport>('potato_usage'),
};

export interface FakeUsageTransport extends PotatoUsageTransport {
  /** How many times the page asked. */
  readonly asked: number;
}

/** The report to answer with, or a plain sentence to fail with. */
export function fakeUsageTransport(answer: UsageReport | string): FakeUsageTransport {
  let asked = 0;
  return {
    get asked() {
      return asked;
    },
    usage: () => {
      asked += 1;
      if (typeof answer === 'string') return Promise.reject(new Error(`POTATO_STORE_INVALID: ${answer}`));
      return Promise.resolve(answer);
    },
  };
}

export const EMPTY_TOTALS: UsageTotals = { messages: 0, tokens: 0, costNano: 0, sandboxActions: 0 };

/** Dollars from nano-dollars, two decimals. A few cents or less shows as "Under $0.01" rather than "$0.00". */
export function moneyText(nano: number): string {
  const value = Math.max(0, nano) / (NANO_PER_CENT * 100);
  if (value > 0 && value < 0.01) return 'Under $0.01';
  return `$${value.toFixed(2)}`;
}

/** Tokens in short words: `812`, `4.2k`, `1.3M`. */
export function tokenText(tokens: number): string {
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)}M`;
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}k`;
  return String(Math.round(tokens));
}

/** A bar's width in percent of its top: 0 to 100, never more, and 0 when there is no top. */
export function barPercent(value: number, top: number): number {
  if (!(top > 0) || !(value > 0)) return 0;
  return Math.min(100, (value / top) * 100);
}

/** The share of a bot's daily limit that today's spend uses. */
export function limitPercent(todayNano: number, limitCents: number): number {
  return barPercent(todayNano, limitCents * NANO_PER_CENT);
}

/** The words under a limit bar: nothing, then a warning at 80%, then the stop at 100%. */
export function limitWord(percent: number): string {
  if (percent >= 100) return 'Limit reached. It stops until tomorrow.';
  if (percent >= 80) return 'Close to the limit.';
  return '';
}

/** `Today`, `Yesterday`, or the short date (`Oct 30`). */
export function dayLabel(date: string, today: string): string {
  if (date === today) return 'Today';
  const parsed = new Date(`${date}T00:00:00`);
  if (Number.isNaN(parsed.getTime())) return date;
  const before = new Date(`${today}T00:00:00`);
  before.setDate(before.getDate() - 1);
  if (before.getTime() === parsed.getTime()) return 'Yesterday';
  return parsed.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
}

/** A plain sentence for a usage read that failed. */
export function usageError(error: unknown): string {
  return plainError(error, 'Could not read the usage.');
}

/** `$0.50 a day` for the limit, reusing the settings wording. */
export function limitText(cents: number): string {
  return `${dollars(cents)} a day`;
}
