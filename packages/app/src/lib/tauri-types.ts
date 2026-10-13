// Payload/result shapes exchanged with the Rust side over Tauri invoke/events.

export type PaneInfo = {
  id: string;
  agent_type: string;
  pid: number;
  status: 'running' | 'waiting_input' | 'idle' | 'error';
  created_at: number;
  last_output_at: number | null;
  cwd: string;
  cols: number;
  rows: number;
  /** A phone viewing this pane holds its size (desktop windows show a notice). */
  viewer_held?: boolean;
  /** The agent reprints its whole conversation on every resize: viewers wipe scrollback at each one. */
  reprints_on_resize?: boolean;
  /** Account id the agent runs under (absent = the CLI's own login) and its label for the header. */
  account?: string | null;
  account_label?: string | null;
};

export type TerminalSnapshotEvent = { pane_id: string; snapshot: string };
export type TerminalDataEvent = { pane_id: string; data: number[] };
export type PaneStatusEvent = { pane_id: string; status: PaneInfo['status'] };
export type PaneExitEvent = { pane_id: string };
export type PanesChangedEvent = { changed: boolean };
/** A phone lease changed a pane's PTY size; ordered with `terminal-data` (desktop owner windows follow it). */
export type PtyResizedEvent = { pane_id: string; cols: number; rows: number };
export type PaneDetachEvent = { pane_id: string; title?: string; cols?: number; rows?: number };
export type PaneReattachEvent = { pane_id: string };

export interface WorkerLaunchConfig {
  agent_type: string;
  cwd?: string | null;
  pane_id?: string | null;
  cols: number;
  rows: number;
  force_new: boolean;
}

export interface EnsureMcpResult {
  installed: boolean;
  changed: boolean;
  backend: string;
  message: string;
}

export interface McpBackendStatus {
  backend: string;
  label: string;
  installed: boolean;
  usesNpm: boolean;
  configPath: string;
  message: string;
}

export interface McpStatusReport {
  bridgeReachable: boolean;
  bridgeUrl: string | null;
  bridgeVersion: string | null;
  portFileExists: boolean;
  portFilePath: string;
  nodeAvailable: boolean;
  npmAvailable: boolean;
  npmPackageVersion: string | null;
  launchCommand: string;
  backends: McpBackendStatus[];
  overallReady: boolean;
  repairResults: EnsureMcpResult[];
}

export interface CoordinationStorageInfo {
  scope: 'project' | 'global_fallback' | string;
  project_path: string | null;
  storage_dir: string;
  event_log_path: string;
  event_count: number;
  task_count: number;
  lock_count: number;
  exists: boolean;
}

export interface OpenCodeKeyProfileInfo {
  id: string;
  label: string;
  configured: boolean;
}

export interface OpenCodeKeyStatus {
  active_profile: string;
  profiles: OpenCodeKeyProfileInfo[];
  key_automation_mode: string;
  restart_pane_on_rotate: boolean;
}

export interface RotateOpenCodeKeyResult {
  ok: boolean;
  active_profile: string;
  profiles: OpenCodeKeyProfileInfo[];
  restarted_panes: string[];
}

export interface AppInstallInfo {
  version: string;
  isPackaged: boolean;
  platform: string;
  uninstallAvailable: boolean;
  uninstallInstructions: string;
  dataDir: string;
}

/** Warm standby pool settings (`standby-pool.json`; see `src-tauri/src/standby`). */
export interface StandbyBackendToggle {
  visible: boolean;
  headless: boolean;
}

/** Settings > Preload: warm visible workers per provider in the default project folder. */
export interface PreloadConfig {
  enabled: boolean;
  /** agent id -> warm workers wanted (0-2); missing or 0 = not warmed. */
  providers: Record<string, number>;
}

export interface StandbyConfig {
  preload: PreloadConfig;
  /** Legacy master switch for the hand-edited headless toggles in `backends`. */
  enabled: boolean;
  backends: Record<string, StandbyBackendToggle>;
  max_total: number;
  recent_projects: number;
  recent_hours: number;
  idle_timeout_secs: number;
}

export interface StandbyView {
  id: number;
  agent_type: string;
  project: string;
  headless: boolean;
  read_only: boolean;
  model: string | null;
  state: 'starting' | 'ready';
  health: string;
  age_ms: number;
  ready_for_ms: number | null;
  idle_expires_in_ms: number | null;
  pane_id?: string;
}

export interface StandbyBlockedView {
  key: string;
  reason: string;
  fails: number;
  retry_in_ms: number;
  cold: boolean;
}

export interface StandbyStatus {
  enabled: boolean;
  config: StandbyConfig;
  counts: { total: number; ready: number; starting: number };
  standbys: StandbyView[];
  blocked: StandbyBlockedView[];
  recent: { key: string; age_ms: number }[];
  stats: {
    spawned: number;
    spawn_failed: number;
    claimed: number;
    missed: number;
    discarded: number;
    expired: number;
    abandoned: number;
  };
  supported: { agent_type: string; visible: boolean; headless: boolean; experimental: boolean }[];
}

export type TunnelMode = 'quick' | 'named' | 'custom';

/** Saved tunnel setting as the UI may see it: never the token, only whether one is stored. */
export interface TunnelConfigView {
  mode: TunnelMode;
  hostname: string | null;
  url: string | null;
  has_token: boolean;
}

export interface TunnelStatus {
  /** Effective mode: `env` when PUPPET_MASTER_PUBLIC_URL overrides the saved setting. */
  mode: TunnelMode | 'env';
  public_url: string | null;
  state: 'stopped' | 'starting' | 'running' | 'error';
  error: string | null;
  config: TunnelConfigView;
}

export type TunnelConfigInput =
  | { mode: 'quick' }
  | { mode: 'named'; hostname: string; token?: string | null }
  | { mode: 'custom'; url: string };

export interface TunnelConnectionTest {
  ok: boolean;
  url: string;
  http_status: number | null;
  latency_ms: number;
  message: string;
}
