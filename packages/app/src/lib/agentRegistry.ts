// Runtime agent registry for the UI (SPEC UI-2): the `agents_list` Tauri command (same body as
// `GET /agents/types`), cached, with the compiled code agents as the offline fallback.
import { invoke } from '@tauri-apps/api/core';
import { presetFor, type AgentType } from '@puppet-master/shared';

export type AgentKind = 'cli' | 'native_protocol' | 'shell' | 'unknown';

/** One registered agent as listed by the backend (SPEC 7.5). */
export interface AgentDescriptor {
  id: AgentType;
  label: string;
  icon: string | null;
  kind: AgentKind | string;
  source: string;
  experimental: boolean;
  dispatchable: boolean;
  not_dispatchable_reason: string | null;
  installed: boolean;
  install_hint: string | null;
  modes: { headless: boolean; visible: boolean };
  features: { read_only_headless: boolean; read_only_spawned_pane: boolean; resumable_headless: boolean };
  run_options: {
    trust_workspace: boolean;
    allow_shell: boolean;
    shell_default_on: boolean;
    allowed_commands: boolean;
  };
  standby: { visible: boolean; headless: boolean };
  definition: unknown | null;
}

export interface AgentRegistryListing {
  agents: AgentDescriptor[];
  dispatchable_ids: AgentType[];
  /** `registry`: answered by the backend; `fallback`: the compiled code agents. */
  source: 'registry' | 'fallback';
}

// [id, kind, dispatchable, headless mode, read_only_headless, read_only_spawned_pane,
//  resumable_headless, trust_workspace, allow_shell, shell_default_on, allowed_commands,
//  standby visible, standby headless] -- mirrors the nine code profiles in registry order.
type FallbackRow = [AgentType, AgentKind, ...boolean[]];
const FALLBACK_ROWS: FallbackRow[] = [
  ['claude', 'cli', true, true, true, false, false, false, false, false, false, true, true],
  ['codex', 'cli', true, true, true, false, true, false, false, false, false, false, false],
  ['opencode', 'cli', true, false, false, false, false, false, false, false, false, false, false],
  ['opencode_native', 'native_protocol', true, false, false, true, false, false, true, true, false, false, false],
  ['cursor_agent', 'cli', true, true, true, false, true, true, true, false, true, true, false],
  ['cursor', 'cli', false, false, false, false, false, false, false, false, false, false, false],
  ['cmd', 'shell', false, false, false, false, false, false, false, false, false, false, false],
  ['powershell', 'shell', false, false, false, false, false, false, false, false, false, false, false],
  ['bash', 'shell', false, false, false, false, false, false, false, false, false, false, false],
];

const NOT_DISPATCHABLE: Record<string, string> = {
  cursor: 'cursor opens the IDE and cannot run delegated work; use cursor_agent for agent runs',
};

/** The code agents compiled into this build, in registry order. */
export const FALLBACK_AGENT_IDS: readonly AgentType[] = FALLBACK_ROWS.map(([id]) => id);

function fallbackDescriptor([id, kind, ...flags]: FallbackRow): AgentDescriptor {
  const [
    dispatchable,
    headless,
    readOnlyHeadless,
    readOnlySpawnedPane,
    resumableHeadless,
    trustWorkspace,
    allowShell,
    shellDefaultOn,
    allowedCommands,
    standbyVisible,
    standbyHeadless,
  ] = flags;
  const preset = presetFor(id);
  return {
    id,
    label: preset.label,
    icon: preset.icon,
    kind,
    source: 'code',
    experimental: false,
    dispatchable,
    not_dispatchable_reason: dispatchable
      ? null
      : (NOT_DISPATCHABLE[id] ?? 'this backend cannot run delegated work'),
    installed: true,
    install_hint: null,
    modes: { headless, visible: true },
    features: {
      read_only_headless: readOnlyHeadless,
      read_only_spawned_pane: readOnlySpawnedPane,
      resumable_headless: resumableHeadless,
    },
    run_options: {
      trust_workspace: trustWorkspace,
      allow_shell: allowShell,
      shell_default_on: shellDefaultOn,
      allowed_commands: allowedCommands,
    },
    standby: { visible: standbyVisible, headless: standbyHeadless },
    definition: null,
  };
}

/** The offline listing: the compiled code agents. */
export function fallbackListing(): AgentRegistryListing {
  const agents = FALLBACK_ROWS.map(fallbackDescriptor);
  return {
    agents,
    dispatchable_ids: agents.filter((agent) => agent.dispatchable).map((agent) => agent.id),
    source: 'fallback',
  };
}

const isTauriRuntime = (): boolean =>
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

function isListingBody(value: unknown): value is Omit<AgentRegistryListing, 'source'> {
  if (!value || typeof value !== 'object') return false;
  const body = value as Record<string, unknown>;
  return (
    Array.isArray(body.agents) &&
    Array.isArray(body.dispatchable_ids) &&
    body.agents.every(
      (agent) => !!agent && typeof (agent as { id?: unknown }).id === 'string',
    )
  );
}

async function fetchListing(): Promise<AgentRegistryListing> {
  if (!isTauriRuntime()) return fallbackListing();
  try {
    const body = await invoke<unknown>('agents_list', undefined);
    if (!isListingBody(body)) return fallbackListing();
    return { agents: body.agents, dispatchable_ids: body.dispatchable_ids, source: 'registry' };
  } catch {
    return fallbackListing();
  }
}

let cached: Promise<AgentRegistryListing> | null = null;

/** Registered agents; cached until `refresh` (or `resetAgentRegistryCache`). Never rejects. */
export function listAgentRegistry(options: { refresh?: boolean } = {}): Promise<AgentRegistryListing> {
  if (!cached || options.refresh) cached = fetchListing();
  return cached;
}

export function resetAgentRegistryCache(): void {
  cached = null;
}
