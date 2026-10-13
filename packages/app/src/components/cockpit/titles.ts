// Pane titles in the cockpit: a derived or user-given title is the primary line, the agent label the secondary.
// Pure helpers, shared by the sidebar, header, inbox and palette (docs/features/ambient-and-titles.md).

/** Longest title the backend stores. */
export const MAX_TITLE_CHARS = 40;

export interface PaneHeading {
  primary: string;
  /** The agent label, shown beside the project when a title is the primary line. */
  secondary: string | null;
}

/** `info.title` as the primary line when present; otherwise the agent name alone (the old behaviour). */
export function paneHeading(info: { title?: string | null }, agentName: string): PaneHeading {
  const title = info.title?.trim();
  return title ? { primary: title, secondary: agentName } : { primary: agentName, secondary: null };
}

/** "Codex · auth refactor" or just the agent name; for places with one line (palette, toasts). */
export function paneOneLine(info: { title?: string | null }, agentName: string): string {
  const title = info.title?.trim();
  return title ? `${agentName} · ${title}` : agentName;
}

/** What the rename box sends: collapsed whitespace, capped. Blank means "clear my name". */
export function cleanTitleInput(raw: string): string {
  return raw.replace(/\s+/g, ' ').trim().slice(0, MAX_TITLE_CHARS).trimEnd();
}
