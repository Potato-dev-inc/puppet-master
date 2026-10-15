// Pure helpers for the pane's chrome: the state line in the header and the dock under the tabs.

/** `Waiting: allow \`npm test\`?` -> `Waiting: allow npm test?`. Raw backticks are never shown in chrome. */
export function plainActivity(text: string): string {
  return text.replace(/`/g, '');
}

/**
 * The header's state line. A pane waiting on a question says what kind of thing it asks
 * ("Needs you · Run tests · 12s"), never the raw command; the command lives in the card's Details.
 */
export function headerStateLine(opts: {
  waiting: boolean;
  headline: string | undefined;
  elapsed: string | null;
  doing: string | null;
  fallback: string;
}): string {
  const { waiting, headline, elapsed, doing, fallback } = opts;
  if (waiting && headline) return ['Needs you', headline, elapsed].filter(Boolean).join(' · ');
  if (doing) return waiting && elapsed ? `${plainActivity(doing)} · ${elapsed}` : plainActivity(doing);
  return fallback;
}

export type DockItem = 'ask' | 'stuck';

/** What is waiting on the person in this pane, most urgent first: the question, then the stuck warning. */
export function dockItems(asking: boolean, stuck: boolean): DockItem[] {
  return [...(asking ? (['ask'] as const) : []), ...(stuck ? (['stuck'] as const) : [])];
}

/** The slim bar shown on the other tabs while something waits in the Terminal tab. */
export const DOCK_LABEL: Record<DockItem, string> = { ask: 'Answer', stuck: 'Open' };
