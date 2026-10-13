// Typed wrapper over Tauri invoke/events with safe no-op fallbacks when running in a plain browser (PWA / dev server).
import { invoke } from '@tauri-apps/api/core';
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window';
import type {
  AgentContextProfile,
  AgentModelInspection,
  PairedDeviceInfo,
  PairingSession,
} from '@puppet-master/shared';

import type {
  PaneInfo,
  TerminalSnapshotEvent,
  TerminalDataEvent,
  PaneStatusEvent,
  PaneExitEvent,
  PanesChangedEvent,
  PtyResizedEvent,
  PaneDetachEvent,
  PaneReattachEvent,
  WorkerLaunchConfig,
  EnsureMcpResult,
  McpStatusReport,
  CoordinationStorageInfo,
  OpenCodeKeyStatus,
  RotateOpenCodeKeyResult,
  AppInstallInfo,
  StandbyConfig,
  StandbyStatus,
} from './tauri-types';

export type * from './tauri-types';

const isTauriRuntime = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const noopUnlisten: UnlistenFn = () => {};

async function safeInvoke<T>(
  command: string,
  args?: Record<string, unknown>,
  fallback?: T,
  hasFallback = false,
): Promise<T> {
  if (!isTauriRuntime()) {
    if (hasFallback) return fallback as T;
    throw new Error(`Tauri command unavailable outside the desktop shell: ${command}`);
  }
  return invoke<T>(command, args);
}

const pendingResizes = new Map<string, Promise<unknown>>();

async function safeListen<T>(event: string, cb: (e: T) => void): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return noopUnlisten;
  return listen<T>(event, (payload) => cb(payload.payload));
}

async function safeEmit<T>(event: string, payload: T): Promise<void> {
  if (!isTauriRuntime()) return;
  await emit(event, payload);
}

async function closeCurrentWindow(): Promise<void> {
  if (!isTauriRuntime()) {
    window.close();
    return;
  }
  await getCurrentWebviewWindow().destroy();
}

async function resizeCurrentWindow(width: number, height: number): Promise<void> {
  if (!isTauriRuntime()) return;
  await getCurrentWindow().setSize(new LogicalSize(width, height));
}

async function onCurrentWindowCloseRequested(cb: () => void | Promise<void>): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return noopUnlisten;
  return getCurrentWindow().onCloseRequested(cb);
}

export const tauri = {
  listPanes: () => safeInvoke<PaneInfo[]>('list_panes', undefined, [], true),
  spawnPane: (args: {
    agent_type: string;
    cwd?: string;
    cols?: number;
    rows?: number;
    extra_args?: string[];
    pane_id?: string;
  }) => safeInvoke<string>('spawn_pane', { args }, `browser-preview-${Date.now()}`, true),
  killPane: (paneId: string) => safeInvoke<void>('kill_pane_cmd', { paneId }, undefined, true),
  killAllPanes: () => safeInvoke<void>('kill_all_panes', undefined, undefined, true),
  writeInput: (
    paneId: string,
    text: string,
    appendNewline = true,
    options?: { viaOpencodeApi?: boolean; modelProvider?: string; modelId?: string },
  ) =>
    safeInvoke<void>(
      'write_pane_input',
      {
        paneId,
        args: {
          text,
          append_newline: appendNewline,
          via_opencode_api: options?.viaOpencodeApi ?? false,
          model_provider: options?.modelProvider,
          model_id: options?.modelId,
        },
      },
      undefined,
      true,
    ),
  readBuffer: (paneId: string, lines: number) =>
    safeInvoke<string>('read_pane_buffer', { paneId, lines }, '', true),
  readSnapshot: (paneId: string) =>
    safeInvoke<string>('read_pane_snapshot', { paneId }, '', true),
  readRawBuffer: (paneId: string, lines: number) =>
    safeInvoke<number[]>('read_pane_raw_buffer', { paneId, lines }, [], true),
  listAgentContexts: () =>
    safeInvoke<AgentContextProfile[]>('list_agent_contexts', undefined, [], true),
  readAgentContext: (args: { agent_type?: string; pane_id?: string }) =>
    safeInvoke<unknown>('read_agent_context', { agentType: args.agent_type, paneId: args.pane_id }),
  inspectAgentModel: (paneId: string, lines?: number) =>
    safeInvoke<AgentModelInspection>('inspect_agent_model', { paneId, lines }),
  switchAgentModel: (paneId: string, args: { model_id: string; model_provider?: string }) =>
    safeInvoke<{ ok: boolean; provider_id: string; model_id: string; tui_synced?: boolean }>(
      'switch_agent_model',
      { paneId, args },
    ),
  readScreenReplay: (paneId: string) =>
    safeInvoke<{ data: number[]; cols: number; rows: number; offset?: number } | null>(
      'read_pane_screen_replay',
      { paneId },
      null,
      true,
    ),
  /** This window's latest resize of the pane while the PTY has not applied it yet. */
  pendingResize: (paneId: string): Promise<unknown> | undefined => pendingResizes.get(paneId),
  resize: (paneId: string, cols: number, rows: number) => {
    const call = safeInvoke<void>('resize_pane', { paneId, cols, rows }, undefined, true);
    const tracked: Promise<unknown> = call.catch(() => undefined).finally(() => {
      if (pendingResizes.get(paneId) === tracked) pendingResizes.delete(paneId);
    });
    pendingResizes.set(paneId, tracked);
    return call;
  },
  /** Take the pane back from a phone: the PTY returns to the size this window last asked for. */
  reclaimPaneSize: (paneId: string) => safeInvoke<void>('reclaim_pane_size', { paneId }, undefined, true),
  setProjectPath: (path: string) => safeInvoke<void>('set_project_path', { path }, undefined, true),
  getProjectPath: () => safeInvoke<string>('get_project_path_cmd', undefined, '', true),
  getCoordinationStorageInfo: () =>
    safeInvoke<CoordinationStorageInfo>(
      'get_coordination_storage_info',
      undefined,
      {
        scope: 'global_fallback',
        project_path: null,
        storage_dir: '.puppet-master',
        event_log_path: '.puppet-master/events.jsonl',
        event_count: 0,
        task_count: 0,
        lock_count: 0,
        exists: false,
      },
      true,
    ),
  ensureOrchestratorMcp: (backend: string, projectPath: string) =>
    safeInvoke<EnsureMcpResult>(
      'ensure_orchestrator_mcp',
      { backend, projectPath },
      {
        installed: true,
        changed: false,
        backend,
        message: 'MCP install skipped in browser preview',
      },
      true,
    ),
  installNpmMcpConfigs: (projectPath: string) =>
    safeInvoke<EnsureMcpResult[]>(
      'install_npm_mcp_configs',
      { projectPath },
      [
        {
          installed: true,
          changed: false,
          backend: 'browser_preview',
          message: 'MCP npm install skipped in browser preview',
        },
      ],
      true,
    ),
  installGlobalNpmMcpConfigs: () =>
    safeInvoke<EnsureMcpResult[]>(
      'install_global_npm_mcp_configs',
      undefined,
      [
        {
          installed: true,
          changed: false,
          backend: 'browser_preview_global',
          message: 'Global MCP npm install skipped in browser preview',
        },
      ],
      true,
    ),
  uninstallNpmMcpConfigs: (projectPath: string) =>
    safeInvoke<EnsureMcpResult[]>(
      'uninstall_npm_mcp_configs',
      { projectPath },
      [
        {
          installed: false,
          changed: true,
          backend: 'browser_preview',
          message: 'MCP uninstall skipped in browser preview',
        },
      ],
      true,
    ),
  uninstallGlobalNpmMcpConfigs: () =>
    safeInvoke<EnsureMcpResult[]>(
      'uninstall_global_npm_mcp_configs',
      undefined,
      [
        {
          installed: false,
          changed: true,
          backend: 'browser_preview_global',
          message: 'Global MCP uninstall skipped in browser preview',
        },
      ],
      true,
    ),
  getMcpStatus: (projectPath: string, autoRepair = false) =>
    safeInvoke<McpStatusReport>(
      'get_mcp_status',
      { projectPath, autoRepair },
      {
        bridgeReachable: false,
        bridgeUrl: null,
        bridgeVersion: null,
        portFileExists: false,
        portFilePath: '',
        nodeAvailable: false,
        npmAvailable: false,
        npmPackageVersion: null,
        launchCommand: 'npx -y @puppet-master/mcp',
        backends: [],
        overallReady: false,
        repairResults: [],
      },
      true,
    ),

  pushChatEvent: (eventJson: string) =>
    safeInvoke<void>('push_chat_event', { eventJson }, undefined, true),
  pushSettingsEvent: (settingsJson: string) =>
    safeInvoke<void>('push_settings_event', { settingsJson }, undefined, true),
  syncPublicSettings: (settingsJson: string) =>
    safeInvoke<void>('sync_public_settings', { settingsJson }, undefined, true),
  onOrchestratorMessage: (cb: (e: { text: string; message_id: string }) => void): Promise<UnlistenFn> =>
    safeListen<{ text: string; message_id: string }>('orchestrator://message', cb),
  onSettingsApply: (cb: (e: { orchestrator_backend?: string; default_provider?: string; default_model?: string; mobile_input_delay_ms?: number; mobile_input_visible?: boolean }) => void): Promise<UnlistenFn> =>
    safeListen<{ orchestrator_backend?: string; default_provider?: string; default_model?: string; mobile_input_delay_ms?: number; mobile_input_visible?: boolean }>('settings://apply', cb),
  onSettingsChanged: (cb: (e: { orchestrator_backend?: string; default_provider?: string; default_model?: string; mobile_input_delay_ms?: number; mobile_input_visible?: boolean }) => void): Promise<UnlistenFn> =>
    safeListen<{ orchestrator_backend?: string; default_provider?: string; default_model?: string; mobile_input_delay_ms?: number; mobile_input_visible?: boolean }>('settings://changed', cb),
  onOrchestratorEnsure: (cb: (e: { backend: string }) => void): Promise<UnlistenFn> =>
    safeListen<{ backend: string }>('orchestrator://ensure', cb),

  onTerminalSnapshot: (cb: (e: TerminalSnapshotEvent) => void): Promise<UnlistenFn> =>
    safeListen<TerminalSnapshotEvent>('terminal-snapshot', cb),
  onTerminalData: (cb: (e: TerminalDataEvent) => void): Promise<UnlistenFn> =>
    safeListen<TerminalDataEvent>('terminal-data', cb),
  onPtyStatus: (cb: (e: PaneStatusEvent) => void): Promise<UnlistenFn> =>
    safeListen<PaneStatusEvent>('pty://status', cb),
  onPtyExit: (cb: (e: PaneExitEvent) => void): Promise<UnlistenFn> =>
    safeListen<PaneExitEvent>('pty://exit', cb),
  onPanesChanged: (cb: (e: PanesChangedEvent) => void): Promise<UnlistenFn> =>
    safeListen<PanesChangedEvent>('pty://panes-changed', cb),
  onPtyResized: (cb: (e: PtyResizedEvent) => void): Promise<UnlistenFn> =>
    safeListen<PtyResizedEvent>('pty://resized', cb),
  onPaneDetach: (cb: (e: PaneDetachEvent) => void): Promise<UnlistenFn> =>
    safeListen<PaneDetachEvent>('pane://detach', cb),
  onPaneShowInGrid: (cb: (e: { pane_id: string }) => void): Promise<UnlistenFn> =>
    safeListen<{ pane_id: string }>('pane://show-in-grid', cb),
  onPaneReattach: (cb: (e: PaneReattachEvent) => void): Promise<UnlistenFn> =>
    safeListen<PaneReattachEvent>('pane://reattach', cb),
  emitPaneReattach: (paneId: string) => safeEmit<PaneReattachEvent>('pane://reattach', { pane_id: paneId }),
  closeCurrentWindow,
  resizeCurrentWindow,
  onCurrentWindowCloseRequested,

  createMobilePairingSession: (bridgeUrl: string) =>
    safeInvoke<PairingSession>(
      'create_mobile_pairing_session',
      { bridgeUrl },
      {
        pairing_code: '',
        expires_at: 0,
        server_public_key: '',
        bridge_url: bridgeUrl,
        qr_payload: { v: 1, u: bridgeUrl, pk: '', c: '', e: 0 },
      },
      true,
    ),
  listPairedMobileDevices: () =>
    safeInvoke<PairedDeviceInfo[]>('list_paired_mobile_devices', undefined, [], true),
  revokePairedMobileDevice: (deviceId: string) =>
    safeInvoke<boolean>('revoke_paired_mobile_device', { deviceId }, false, true),

  getMobileTunnelInfo: () =>
    safeInvoke<{
      localUrl: string;
      pwaUrl: string;
      tunnelUrl?: string | null;
      tunnelProvider?: string | null;
      bridgeProxyUrl: string;
      bridgeDirectUrl?: string | null;
      updatedAt: number;
    } | null>('get_mobile_tunnel_info', undefined, null, true),

  standbyGetStatus: () => safeInvoke<StandbyStatus | null>('standby_get_status', undefined, null, true),
  standbySetConfig: (config: StandbyConfig) =>
    safeInvoke<StandbyStatus | null>('standby_set_config', { config }, null, true),
  getAppInstallInfo: () =>
    safeInvoke<AppInstallInfo>(
      'get_app_install_info',
      undefined,
      {
        version: '0.0.0',
        isPackaged: false,
        platform: 'browser',
        uninstallAvailable: false,
        uninstallInstructions: '',
        dataDir: '',
      },
      true,
    ),
  openExternalUrl: (url: string) =>
    safeInvoke<void>('open_external_url', { url }, undefined, true),
  launchUninstall: () => safeInvoke<void>('launch_uninstall', undefined, undefined, true),
  getWorkerLaunch: () => safeInvoke<WorkerLaunchConfig | null>('get_worker_launch', undefined, null, true),
  getOpenCodeKeyStatus: () =>
    safeInvoke<OpenCodeKeyStatus>(
      'get_opencode_key_status',
      undefined,
      {
        active_profile: 'a',
        profiles: [
          { id: 'a', label: 'Primary', configured: false },
          { id: 'b', label: 'Secondary', configured: false },
        ],
        key_automation_mode: 'auto_if_backup',
        restart_pane_on_rotate: true,
      },
      true,
    ),
  setOpenCodeAutomationSettings: (mode: 'auto_if_backup' | 'notify_only', restartPaneOnRotate: boolean) =>
    safeInvoke<OpenCodeKeyStatus>(
      'set_opencode_automation_settings',
      { mode, restartPaneOnRotate },
      {
        active_profile: 'a',
        profiles: [],
        key_automation_mode: mode,
        restart_pane_on_rotate: restartPaneOnRotate,
      },
      true,
    ),
  setOpenCodeKeyProfile: (profileId: string, apiKey: string, label?: string) =>
    safeInvoke<void>(
      'set_opencode_key_profile',
      { profileId, apiKey, label: label ?? null },
      undefined,
      true,
    ),
  captureOpenCodeKeyProfile: (profileId: string, label?: string) =>
    safeInvoke<void>(
      'capture_opencode_key_profile',
      { profileId, label: label ?? null },
      undefined,
      true,
    ),
  rotateOpenCodeKey: (profile: 'next' | 'a' | 'b', paneId?: string) =>
    safeInvoke<RotateOpenCodeKeyResult>(
      'rotate_opencode_key',
      { profile, paneId: paneId ?? null },
      { ok: true, active_profile: 'a', profiles: [], restarted_panes: [] },
      true,
    ),
  exitApp: () => safeInvoke<void>('exit_app', undefined, undefined, true),
};
