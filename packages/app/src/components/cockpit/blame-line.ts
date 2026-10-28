import type { ChangeBlame } from '../../lib/cockpit-api';

/** "14:02" on the local clock. */
export function clockTime(atMs: number): string {
  const d = new Date(atMs);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

/** The gray line next to a changed file ("Codex helper · 14:02") and its hover text. */
export function blameLine(blame: ChangeBlame): { text: string; title: string } {
  const time = clockTime(blame.atMs);
  const who = blame.paneTitle ? `${blame.agent} ${blame.paneTitle}` : blame.agent;
  return { text: `${who} \u00b7 ${time}`, title: `Changed by ${blame.agent} in turn at ${time}` };
}
