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
        case 'list_agent_contexts':
          return [];
        case 'pane_set_title':
          return String(a.title ?? '') || null;
        case 'ambient_get_prefs':
          return { animated_tray_icon: true };
        case 'ambient_set_animated_tray':
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
