// The words for usage in the Helpers card: "Helpers: 3 · 14 turns · 11 min" and under each helper
// "Codex · 6 turns · 4 min · work account · 12k in, 3k out tokens". Pure, no React. A piece we do not
// have is left out, never guessed: no tokens unless the chat reported them, no account unless known.
import type { HelperUsage, PaneUsage, UsageTokens } from '../../../lib/pane-usage';
import { formatTokens } from './railFormat';

const plural = (n: number, unit: string): string => `${n} ${unit}${n === 1 ? '' : 's'}`;

/** "0 min", "under 1 min", "4 min", "1 h 5 min". */
export function busyText(ms: number): string {
  const minutes = Math.floor(ms / 60_000);
  if (minutes < 1) return ms > 0 ? 'under 1 min' : '0 min';
  if (minutes < 60) return `${minutes} min`;
  return `${Math.floor(minutes / 60)} h ${minutes % 60} min`;
}

export function tokensText(tokens: UsageTokens): string {
  return `${formatTokens(tokens.input)} in, ${formatTokens(tokens.output)} out tokens`;
}

/** One helper's line: "Codex · 6 turns · 4 min · work account · 12k in, 3k out tokens". */
export function helperUsageLine(agent: string | null | undefined, usage: HelperUsage): string {
  return [
    agent,
    plural(usage.turns, 'turn'),
    busyText(usage.busyMs),
    usage.accountLabel ? `${usage.accountLabel} account` : null,
    usage.tokens ? tokensText(usage.tokens) : null,
  ]
    .filter(Boolean)
    .join(' · ');
}

/** The parent's roll-up: "Helpers: 3 · 14 turns · 11 min". */
export function rollupLine(usage: PaneUsage): string {
  return `Helpers: ${usage.helperCount} · ${plural(usage.helperTurns, 'turn')} · ${busyText(usage.helperBusyMs)}`;
}
