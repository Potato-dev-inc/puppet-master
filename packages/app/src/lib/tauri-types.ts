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
};

export type TerminalSnapshotEvent = { pane_id: string; snapshot: string };
export type TerminalDataEvent = { pane_id: string; data: number[] };
export type PaneStatusEvent = { pane_id: string; status: PaneInfo['status'] };
export type PaneExitEvent = { pane_id: string };
export type PanesChangedEvent = { changed: boolean };
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
