// Mocks the Tauri IPC layer so the real UI runs in a plain browser.
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import type { HarnessScenario } from './types';
import {
  EXAMPLE_CHECKPOINTS,
  EXAMPLE_FILE_CHANGES,
  EXAMPLE_PANES,
  EXAMPLE_PROMPT,
  HIGH_RISK_PROMPT,
  PANE_CODEX,
  EXAMPLE_RUN,
  EXAMPLE_WORKTREE,
  EXAMPLE_WORKTREE_DIFF,
  EXAMPLE_TRANSCRIPT,
  PANE_CLAUDE,
  SCREEN_REPLAY,
  ACCOUNT_AGENTS,
  agentsListing,
  TOOLBELT_IMPORT,
  TOOLBELT_LISTING,
  TOOLBELT_PREVIEW,
  standbyStatus,
} from './fixtures';

const DOCTOR_ROWS = [
  { id: 'claude', label: 'Claude Code', state: 'unchecked', installed: true, version: '2.1.4 (Claude Code)', reason: 'Installed. Press Test to check it can answer.', install_hint: 'npm i -g @anthropic-ai/claude-code', login_hint: 'Open Claude Code and type /login', can_test: true },
  { id: 'codex', label: 'Codex', state: 'needs_login', installed: true, version: 'codex-cli 0.9.0', reason: 'It needs you to sign in first.', install_hint: 'npm i -g @openai/codex', login_hint: 'Run: codex login', can_test: true },
  { id: 'opencode', label: 'OpenCode', state: 'not_installed', installed: false, version: null, reason: 'Not installed on this computer.', install_hint: 'npm i -g opencode-ai', login_hint: 'Open OpenCode and type /connect', can_test: false },
  { id: 'cursor_agent', label: 'Cursor Agent', state: 'error', installed: true, version: null, reason: 'Found it, but it did not start properly: exit code 1', install_hint: null, login_hint: 'Run: agent login', can_test: true },
];

export function installTauriMock(scenario: HarnessScenario): void {
  const panes = scenario.panes ?? EXAMPLE_PANES;
  // In-memory plugin-store (one store, rid 1), keyed as the app writes it.
  const store = new Map<string, unknown>([['settings', { theme: scenario.theme ?? 'dark' }]]);

  mockWindows(scenario.route === 'settings' ? 'settings' : 'cockpit');
  mockIPC(
    (cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      switch (cmd) {
        case 'list_panes':
          return panes;
        case 'accounts_list':
          return { agents: ACCOUNT_AGENTS, accounts: scenario.accounts ?? [] };
        case 'new_pane':
          return 'pane-new-1';
        case 'pane_prompt':
          return a.paneId === PANE_CLAUDE ? EXAMPLE_PROMPT : a.paneId === PANE_CODEX ? HIGH_RISK_PROMPT : null;
        case 'get_ui_mode':
          return 'orchestration';
        case 'standby_get_status':
          return standbyStatus();
        case 'agents_list':
          return agentsListing();
        case 'pane_run_info':
          return a.paneId === PANE_CLAUDE ? EXAMPLE_RUN : null;
        case 'pane_worktree':
          return a.paneId === PANE_CLAUDE ? EXAMPLE_WORKTREE : null;
        case 'worktree_diff':
          return EXAMPLE_WORKTREE_DIFF;
        case 'pane_transcript':
          return a.paneId === PANE_CLAUDE ? EXAMPLE_TRANSCRIPT : null;
        case 'checkpoints_list':
          return a.paneId === PANE_CLAUDE ? EXAMPLE_CHECKPOINTS : { state: 'ok', message: '', items: [] };
        case 'checkpoint_diff':
          return EXAMPLE_FILE_CHANGES;
        case 'checkpoint_restore':
        case 'checkpoint_undo_last':
          return { safetyId: '7', written: 4, removed: 1, failed: [] };
        case 'read_pane_screen_replay':
          return SCREEN_REPLAY;
        case 'read_pane_raw_buffer':
          return SCREEN_REPLAY.data;
        case 'read_pane_buffer':
        case 'read_pane_snapshot':
          return '';
        case 'get_project_path_cmd':
          return 'C:\\Users\\dev\\work\\puppet-master';
        case 'pane_activities':
          return Object.fromEntries(
            panes.map((p) => [
              p.id,
              p.status === 'waiting_input'
                ? { kind: 'waiting', text: 'Waiting: allow `npm test`?' }
                : p.status === 'running'
                  ? { kind: 'working', text: 'Editing src/app.ts' }
                  : { kind: 'idle', text: 'Idle' },
            ]),
          );
        case 'pane_previews':
          return {
            [PANE_CODEX]: [
              { url: 'http://localhost:5173/', port: 5173, networkUrl: 'http://192.168.1.23:5173/' },
              { url: 'http://localhost:3000', port: 3000 },
            ],
          };
        case 'loop_signals':
          return scenario.name === 'loop-stuck'
            ? [
                {
                  pane_id: PANE_CLAUDE,
                  kind: 'repeated_error',
                  evidence_lines: [
                    'error[E0432]: unresolved import `crate::store::Cache`',
                    'error: could not compile `app` due to 1 previous error',
                  ],
                  turns: 4,
                  since: 0,
                  summary: 'the same error 4 times',
                },
              ]
            : [];
        case 'loop_settings_get':
          return { enabled: true, turns: 4, stalled_minutes: 20, panic_shortcut: false };
        case 'stop_all_agents':
          return {
            stopped: panes
              .filter((p) => p.status === 'running')
              .map((p) => ({ pane_id: p.id, agent_type: p.agent_type, cwd: p.cwd })),
            failed: [],
          };
        case 'agents_health':
          return { agents: DOCTOR_ROWS };
        case 'agents_health_test':
          return { ...DOCTOR_ROWS[0], state: 'ready', reason: 'Ready. It answered the test question.' };
        case 'list_agent_contexts':
          return [];
        case 'secret_storage_info':
          return { secure: true, label: 'Windows Credential Manager' };
        case 'secret_get_masked':
          return a.name === 'anthropic_api_key' ? { set: true, last4: 'WXYZ' } : { set: false, last4: null };
        case 'get_opencode_key_status':
          return { active_profile: 'a', profiles: [{ id: 'a', label: 'Primary', configured: true }, { id: 'b', label: 'Secondary', configured: false }], key_automation_mode: 'auto_if_backup', restart_pane_on_rotate: true };
        case 'questions_list':
          return scenario.questions ?? [];
        case 'question_answer':
        case 'question_dismiss':
          return { accepted: true, question: (scenario.questions ?? []).find((q) => q.id === a.id) };
        case 'get_tunnel_status':
          return {
            mode: 'named',
            public_url: 'https://pm.example.com',
            state: 'running',
            error: null,
            config: { mode: 'named', hostname: 'pm.example.com', url: null, has_token: true },
          };
        case 'test_tunnel_connection':
          return { ok: true, url: 'https://pm.example.com/bridge/health', http_status: 200, latency_ms: 84, message: 'Connected in 84 ms.' };
        case 'list_paired_mobile_devices':
          return [];
        case 'get_mobile_tunnel_info':
          return { localUrl: 'http://127.0.0.1:1420', pwaUrl: 'http://127.0.0.1:1420', tunnelUrl: 'https://pm.example.com', bridgeProxyUrl: 'https://pm.example.com/bridge', updatedAt: 0 };
        case 'pane_set_title':
          return String(a.title ?? '') || null;
        case 'ambient_get_prefs':
          return { animated_tray_icon: true };
        case 'ambient_set_animated_tray':
        case 'toolbelt_list':
          return TOOLBELT_LISTING;
        case 'toolbelt_sync_preview':
          return TOOLBELT_PREVIEW;
        case 'toolbelt_sync_apply':
          return { ...TOOLBELT_PREVIEW, dry_run: false };
        case 'toolbelt_import_preview':
          return { candidates: TOOLBELT_IMPORT };
        case 'toolbelt_import_apply':
          return { added: [] };
        case 'resize_pane':
        case 'set_ui_mode':
        case 'interrupt_pane':
        case 'answer_pane_prompt':
          return null;
        // plugin-store
        case 'plugin:store|get_store':
        case 'plugin:store|load':
          return 1;
        case 'plugin:store|get': {
          const key = String(a.key);
          return [store.get(key) ?? null, store.has(key)];
        }
        case 'plugin:store|set':
          store.set(String(a.key), a.value);
          return null;
        case 'plugin:store|has':
          return store.has(String(a.key));
        case 'plugin:store|keys':
          return [...store.keys()];
        case 'plugin:store|save':
          return null;
        default:
          if (cmd.startsWith('plugin:event|') || cmd.startsWith('plugin:window|') || cmd.startsWith('plugin:path|')) {
            return null;
          }
          console.warn(`[harness] unmocked Tauri command: ${cmd}`, args);
          return null;
      }
    },
    { shouldMockEvents: true },
  );
}
