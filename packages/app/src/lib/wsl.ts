// Linux Inside: typed wrappers for the WSL commands (`commands/wsl.rs`, desktop window only). The types
// mirror `wsl/model.rs` and the spawn args mirror `SpawnPaneArgs` (snake_case, as Rust serialises it).
// The Rust side decides everything; nothing here starts a distro or changes WSL.
import { invoke } from '@tauri-apps/api/core';

/** Mirrors `WslStatus` (`#[serde(tag = "state", rename_all = "snake_case")]`). */
export type WslStatus =
  | { state: 'not_windows' }
  /** `command` is always `wsl --install` (a Copy button only). */
  | { state: 'not_installed'; command: string }
  /** `command` is always `wsl --install -d Ubuntu`. */
  | { state: 'no_distro'; command: string }
  | { state: 'ready'; distros: Distro[] }
  /** First non-empty output line, redacted, at most 200 characters. */
  | { state: 'broken'; line: string };

/** Mirrors `Distro` (camelCase). `state` is the word `wsl.exe` printed (localised; display only). */
export interface Distro {
  name: string;
  default: boolean;
  version: number;
  state: string;
}

/** Mirrors `DistroView` (camelCase, spec 3.7). Holds no PATH, uid or home. */
export interface DistroView {
  name: string;
  version: number;
  default: boolean;
  checkedAtMs: number;
  git: string | null;
  interop: boolean;
  networking: string | null;
  agents: AgentPlaceRow[];
}

/** Mirrors `AgentPlaceRow` (camelCase). `distro: null` means Windows. */
export interface AgentPlaceRow {
  agentId: string;
  label: string;
  distro: string | null;
  /** `null` means that distro was not checked yet. */
  found: boolean | null;
  /** A plain sentence when Linux cannot be chosen. */
  whyNot: string | null;
}

/** Mirrors `PanePlace` on `PaneInfo` (snake_case). Display and chat facts; grants nothing. */
export interface PanePlace {
  /** `linux`. */
  kind: string;
  distro: string;
  /** The folder as the agent sees it, for example `/mnt/d/work/shop`. */
  cwd: string;
  /** For the Linux -> Windows path mapping, for example `/mnt/`. */
  mount_root: string;
  /** For example `\\wsl.localhost\Ubuntu`. */
  unc_root: string;
  home_windows?: string;
  warnings?: string[];
}

/** Mirrors the JSON fields of `SpawnPaneArgs` (`pty/registry/command.rs`). The guard env is never JSON. */
export interface SpawnPaneArgs {
  agent_type: string;
  cwd?: string;
  cols?: number;
  rows?: number;
  extra_args?: string[];
  pane_id?: string;
  read_only?: boolean;
  account?: string;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<T>(command, args);
}

export const tauriWslApi = {
  /** List only (never boots a distro). `refresh` skips the 30 s cache. */
  status: (refresh: boolean) => call<WslStatus>('wsl_status', { refresh }),
  /** Boots the distro and probes it (up to 25 s). Only on a person action. */
  check: (distro: string) => call<DistroView>('wsl_check', { distro }),
  placesGet: () => call<AgentPlaceRow[]>('wsl_places_get'),
  placeSet: (agentId: string, distro: string | null) => call<AgentPlaceRow>('wsl_place_set', { agentId, distro }),
  /** Starts a pane for one agent in the given distro, whatever the saved setting says. */
  spawnInLinux: (args: SpawnPaneArgs, distro: string) => call<string>('spawn_pane_in_linux', { args, distro }),
};

export type WslApi = typeof tauriWslApi;

export function wslStatus(refresh: boolean, api: WslApi = tauriWslApi): Promise<WslStatus> {
  return api.status(refresh);
}

export function wslCheck(distro: string, api: WslApi = tauriWslApi): Promise<DistroView> {
  return api.check(distro);
}

export function wslPlacesGet(api: WslApi = tauriWslApi): Promise<AgentPlaceRow[]> {
  return api.placesGet();
}

/** `distro: null` sets the agent back to Windows (always allowed). */
export function wslPlaceSet(agentId: string, distro: string | null, api: WslApi = tauriWslApi): Promise<AgentPlaceRow> {
  return api.placeSet(agentId, distro);
}

export function spawnPaneInLinux(args: SpawnPaneArgs, distro: string, api: WslApi = tauriWslApi): Promise<string> {
  return api.spawnInLinux(args, distro);
}
