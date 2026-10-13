// Mocks the Tauri IPC layer so the real UI runs in a plain browser.
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import type { HarnessScenario } from './types';
import {
  EXAMPLE_PANES,
  EXAMPLE_PROMPT,
  EXAMPLE_RUN,
  EXAMPLE_TRANSCRIPT,
  PANE_CLAUDE,
  SCREEN_REPLAY,
  agentsListing,
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

  mockWindows(scenario.route === 'settings' ? 'settings' : scenario.route === 'askbar' ? 'askbar' : 'cockpit');
  mockIPC(
    (cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      switch (cmd) {
        case 'list_panes':
          return panes;
        case 'pane_prompt':
          return a.paneId === PANE_CLAUDE ? EXAMPLE_PROMPT : null;
        case 'get_ui_mode':
          return 'orchestration';
        case 'standby_get_status':
          return standbyStatus();
        case 'agents_list':
          return agentsListing();
        case 'pane_run_info':
          return a.paneId === PANE_CLAUDE ? EXAMPLE_RUN : null;
        case 'pane_transcript':
          return a.paneId === PANE_CLAUDE ? EXAMPLE_TRANSCRIPT : null;
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
        case 'askbar_settings_get':
          return { enabled: true, shortcut: 'Ctrl+Alt+Space', default_agent: 'claude', last_agent: null };
        case 'askbar_settings_set':
          return a.settings;
        case 'askbar_agents':
          return [
            { id: 'claude', label: 'Claude Code', ready: true },
            { id: 'codex', label: 'Codex', ready: false },
            { id: 'cursor_agent', label: 'Cursor Agent', ready: false },
          ];
        case 'askbar_clipboard':
          return { text: 'x'.repeat(1200), chars: 1200, truncated: false };
        case 'askbar_resize':
        case 'askbar_hide':
        case 'askbar_continue':
        case 'askbar_notice_dismiss':
        case 'askbar_notice_open':
          return null;
        case 'askbar_notices':
          return [];
        case 'askbar_ask': {
          const req = a.request as { ask_id: string; question: string };
          return new Promise((resolve) =>
            window.setTimeout(
              () =>
                resolve({
                  ask_id: req.ask_id,
                  agent: 'claude',
                  agent_label: 'Claude Code',
                  question: req.question,
                  ok: true,
                  answer:
                    'Short answer: use a `Mutex<HashMap<..>>` only for the index and keep the rows in an `Arc`.\n\n```rust\nlet index = Arc::new(Mutex::new(HashMap::new()));\nindex.lock().insert(id, row);\n```\n\nThat keeps the lock short and the reads cheap.',
                  error: null,
                }),
              400,
            ),
          );
        }
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
