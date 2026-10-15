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

interface LoginInfo {
  agent_type: string;
  account?: string | null;
}

/** Agent types that have panes under more than one login (the CLI's own login counts as one). */
export function multiLoginAgents(panes: ReadonlyArray<LoginInfo>): Set<string> {
  const seen = new Map<string, Set<string>>();
  for (const p of panes) {
    const logins = seen.get(p.agent_type) ?? new Set<string>();
    logins.add(p.account ?? '');
    seen.set(p.agent_type, logins);
  }
  return new Set([...seen].filter(([, logins]) => logins.size > 1).map(([agent]) => agent));
}

/**
 * The second line of a pane row: who is working and what it is doing, with the login when that
 * agent runs under several ("Codex · Editing src/app.ts · Work"). An untitled pane already shows
 * the agent name as its first line, so its second line leads with the detail.
 */
export function paneSubline(
  info: LoginInfo & { title?: string | null; account_label?: string | null },
  agentName: string,
  detail: string,
  multiLogin: ReadonlySet<string>,
): string {
  const parts: string[] = [];
  if (info.title?.trim()) parts.push(agentName);
  parts.push(detail);
  const login = info.account_label?.trim();
  if (login && multiLogin.has(info.agent_type)) parts.push(login);
  return parts.join(' · ');
}

/** What the rename box sends: collapsed whitespace, capped. Blank means "clear my name". */
export function cleanTitleInput(raw: string): string {
  return raw.replace(/\s+/g, ' ').trim().slice(0, MAX_TITLE_CHARS).trimEnd();
}
