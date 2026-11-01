// The chat add-on (the small helper program a Chat session needs) and the sign-in it asks for. Typed commands for
// the desktop app, plus the pure rules the screens share: the install prompt's words, the Settings row's words, and how
// a start error is told apart. Nothing here names an agent: labels, sizes and sign-in hints come from the backend.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type AddonState = 'not_installed' | 'installing' | 'installed' | 'failed';

/** One agent's add-on, as `acp_addon_status` reports it. */
export interface AddonStatus {
  agent: string;
  label: string;
  state: AddonState;
  version: string | null;
  pinned_version: string | null;
  /** Why it failed, in a plain sentence. */
  detail: string;
  size_mb: number | null;
  /** While installing: the newest progress lines. */
  lines: string[];
  /** What the sign-in card says for this agent. */
  signin_hint: string;
}

export interface AddonList {
  labsEnabled: boolean;
  addons: AddonStatus[];
}

export interface AcpAddonApi {
  list: () => Promise<AddonList>;
  install: (agentId: string) => Promise<AddonStatus>;
  remove: (agentId: string) => Promise<AddonStatus>;
  /** Called when an install makes progress or ends. Returns the stop function. */
  watch: (cb: (status: AddonStatus) => void) => Promise<UnlistenFn>;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const noopUnlisten: UnlistenFn = () => {};

interface ListWire {
  labs_enabled?: boolean;
  addons?: unknown;
}

const STATES: readonly AddonState[] = ['not_installed', 'installing', 'installed', 'failed'];

/** One status from the wire, or null when it is not one. */
export function parseStatus(raw: unknown): AddonStatus | null {
  if (!raw || typeof raw !== 'object') return null;
  const r = raw as Record<string, unknown>;
  if (typeof r.agent !== 'string' || !STATES.includes(r.state as AddonState)) return null;
  const str = (v: unknown): string => (typeof v === 'string' ? v : '');
  return {
    agent: r.agent,
    label: str(r.label) || r.agent,
    state: r.state as AddonState,
    version: typeof r.version === 'string' ? r.version : null,
    pinned_version: typeof r.pinned_version === 'string' ? r.pinned_version : null,
    detail: str(r.detail),
    size_mb: typeof r.size_mb === 'number' ? r.size_mb : null,
    lines: Array.isArray(r.lines) ? r.lines.filter((l): l is string => typeof l === 'string') : [],
    signin_hint: str(r.signin_hint),
  };
}

export function parseList(wire: ListWire | null | undefined): AddonList {
  const addons = Array.isArray(wire?.addons) ? wire!.addons.map(parseStatus).filter((s): s is AddonStatus => s !== null) : [];
  return { labsEnabled: wire?.labs_enabled === true, addons };
}

const wireStatus = (raw: unknown): AddonStatus => {
  const status = parseStatus(raw);
  if (!status) throw new Error('The app sent an add-on status this screen cannot read.');
  return status;
};

export const tauriAcpAddonApi: AcpAddonApi = {
  list: async () => {
    if (!isTauri()) return { labsEnabled: false, addons: [] };
    try {
      return parseList(await invoke<ListWire>('acp_addon_status'));
    } catch {
      return { labsEnabled: false, addons: [] }; // an older build without the command
    }
  },
  install: async (agentId) => {
    if (!isTauri()) throw new Error('Desktop shell required: acp_addon_install');
    return wireStatus(await invoke<unknown>('acp_addon_install', { agentId }));
  },
  remove: async (agentId) => {
    if (!isTauri()) throw new Error('Desktop shell required: acp_addon_remove');
    return wireStatus(await invoke<unknown>('acp_addon_remove', { agentId }));
  },
  watch: async (cb) => {
    if (!isTauri()) return noopUnlisten;
    return listen<{ agent: string; status: unknown }>('acp-addon-changed', (e) => {
      const status = parseStatus(e.payload?.status);
      if (status) cb(status);
    });
  },
};

// ---------- the words ----------

/** The add-on of `agentId` (ids compare without case, like the lane list does), if the agent has one. */
export function addonFor(byAgent: Record<string, AddonStatus>, agentId: string | null | undefined): AddonStatus | undefined {
  if (!agentId) return undefined;
  const wanted = agentId.toLowerCase();
  return Object.values(byAgent).find((a) => a.agent.toLowerCase() === wanted);
}

/** "about 48 MB", or nothing when the size is not known. */
export function sizeText(mb: number | null): string {
  return mb && mb > 0 ? ` (about ${mb} MB)` : '';
}

/** The install prompt: "Chat needs a small add-on for Claude (about 48 MB)." */
export function promptText(s: AddonStatus): string {
  return `Chat needs a small add-on for ${s.label}${sizeText(s.size_mb)}.`;
}

/** The newest progress line, or a plain "Installing…". */
export function progressText(s: AddonStatus): string {
  const last = s.lines.length > 0 ? s.lines[s.lines.length - 1] : '';
  return last || 'Installing…';
}

/** Whether Chat for this agent still needs the add-on installed (so Send waits and the prompt shows). */
export function needsInstall(s: AddonStatus | undefined | null): boolean {
  return !!s && s.state !== 'installed';
}

/** The Settings row's status sentence. */
export function rowText(s: AddonStatus): string {
  switch (s.state) {
    case 'installed':
      return `Installed${s.version ? ` ${s.version}` : ''}`;
    case 'installing':
      return `Installing… ${s.lines.length > 0 ? progressText(s) : ''}`.trim();
    case 'failed':
      return s.detail || 'The install did not finish.';
    default:
      return 'Not installed';
  }
}

// ---------- telling a start error apart ----------

/** What a chat start (or retry) error means for the screen. */
export type ChatBlock = 'addon' | 'signin';

/** `ACP_ADDON_MISSING: …` and `ACP_SIGN_IN_NEEDED: …` are the two the screen answers with a card. */
export function blockOf(message: string | null | undefined): ChatBlock | null {
  const text = (message ?? '').trimStart();
  if (text.startsWith('ACP_ADDON_MISSING')) return 'addon';
  if (text.startsWith('ACP_SIGN_IN_NEEDED')) return 'signin';
  return null;
}

/** The sentence after `CODE: `, for showing to the person. */
export function plainMessage(message: string): string {
  return message.replace(/^[A-Z][A-Z0-9_]{2,}:\s*/, '');
}
