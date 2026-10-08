// Runtime agent registry for the UI (SPEC UI-2): the `agents_list` Tauri command (same body as
// `GET /agents/types`), cached, with the compiled code agents as the offline fallback.
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listLaunchPresets, presetFor, type AgentType } from '@puppet-master/shared';

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
let resolved: AgentRegistryListing | null = null;

/** Registered agents; cached until `refresh` (or `resetAgentRegistryCache`). Never rejects. */
export function listAgentRegistry(options: { refresh?: boolean } = {}): Promise<AgentRegistryListing> {
  if (!cached || options.refresh) {
    const pending = fetchListing();
    cached = pending;
    void pending.then((listing) => {
      if (cached === pending) resolved = listing;
    });
  }
  return cached;
}

export function resetAgentRegistryCache(): void {
  cached = null;
  resolved = null;
}

/** The last resolved listing (synchronous), or the compiled fallback before the first fetch settles. */
export function peekAgentRegistry(): AgentRegistryListing {
  return resolved ?? fallbackListing();
}

/** React hook: the compiled fallback on first render, then the registry listing once it resolves. */
export function useAgentRegistry(): AgentRegistryListing {
  const [listing, setListing] = useState<AgentRegistryListing>(peekAgentRegistry);
  useEffect(() => {
    let live = true;
    void listAgentRegistry().then((next) => {
      if (live) setListing(next);
    });
    return () => {
      live = false;
    };
  }, []);
  return listing;
}

export interface LaunchEntry {
  type: AgentType;
  label: string;
  icon: string | null;
}

/**
 * Spawn-menu entries: the compiled launch presets (OpenCode API, PowerShell) that the registry still
 * lists, followed by any registered agent that is not a code profile and can open a visible pane.
 */
export function launchMenuEntries(listing: AgentRegistryListing): LaunchEntry[] {
  const byId = new Map(listing.agents.map((agent) => [agent.id, agent]));
  const entries: LaunchEntry[] = [];
  for (const preset of listLaunchPresets()) {
    const agent = byId.get(preset.type);
    if (agent) entries.push({ type: agent.id, label: agent.label || preset.label, icon: agent.icon });
  }
  for (const agent of listing.agents) {
    if (agent.source === 'code' || !agent.modes.visible || entries.some((e) => e.type === agent.id)) continue;
    entries.push({ type: agent.id, label: agent.label, icon: agent.icon });
  }
  return entries;
}

/** Display label for an agent id; the id itself when the registry does not know it. */
export function agentLabel(listing: AgentRegistryListing, id: string): string {
  return listing.agents.find((agent) => agent.id === id)?.label ?? id;
}

const COMPILED_TUI = new Set<string>(['claude', 'codex', 'opencode', 'opencode_native']);

/** Does this agent drive a TUI (permission prompts, ready detection)? Registry-aware, compiled set as base. */
export function isTuiAgentId(id: string): boolean {
  if (COMPILED_TUI.has(id)) return true;
  if (FALLBACK_AGENT_IDS.includes(id)) return false;
  const agent = peekAgentRegistry().agents.find((a) => a.id === id);
  return !!agent && (agent.kind === 'cli' || agent.kind === 'native_protocol');
}
