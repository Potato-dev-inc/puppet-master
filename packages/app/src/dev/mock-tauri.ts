// Mocks the Tauri IPC layer so the real UI runs in a plain browser.
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import type { HarnessScenario } from './types';
import {
  EXAMPLE_CHECKPOINTS,
  EXAMPLE_DIGEST,
  EXAMPLE_TURN_SUMMARIES,
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
  AGENT_PACK_PREVIEW,
  TOOLBELT_LISTING,
  TOOLBELT_PREVIEW,
  standbyStatus,
  EXAMPLE_REVIEWERS,
  EXAMPLE_REVIEW_START,
  EXAMPLE_REVIEW_POLL,
} from './fixtures';
import { TEAM_DIFF, teamConfig } from './team-fixtures';
import { shipMock } from './ship-fixtures';

const DOCTOR_ROWS = [
  { id: 'claude', label: 'Claude Code', state: 'unchecked', installed: true, version: '2.1.4 (Claude Code)', reason: 'Installed. Press Test to check it can answer.', install_hint: 'winget install Anthropic.ClaudeCode', install_fallback: 'npm i -g @anthropic-ai/claude-code', login_hint: 'Open Claude Code and type /login', can_test: true, hooks: { state: 'connected', mode: 'shadow', sentence: 'Connected. Claude Code tells Puppet Master when it starts, asks and finishes. Watch only: Puppet Master still decides from the screen.' } },
  { id: 'codex', label: 'Codex', state: 'needs_login', installed: true, version: 'codex-cli 0.162.1', reason: 'It needs you to sign in first.', install_hint: 'powershell -ExecutionPolicy ByPass -c "irm https://chatgpt.com/codex/install.ps1 | iex"', install_fallback: 'npm i -g @openai/codex', login_hint: 'Run: codex login', can_test: true, hooks: { state: 'off', mode: 'off', sentence: 'Live updates are off.', opt_in: true, opt_in_note: 'Puppet Master adds a small file to .codex in each project; Codex asks you to review it once per folder.', enabled: false } },
  { id: 'opencode', label: 'OpenCode', state: 'not_installed', installed: false, version: null, reason: 'Not installed on this computer.', install_hint: 'npm i -g opencode-ai', login_hint: 'Open OpenCode and type /connect', can_test: false },
  { id: 'cursor_agent', label: 'Cursor Agent', state: 'error', installed: true, version: null, reason: 'Found it, but it did not start properly: exit code 1', install_hint: null, login_hint: 'Run: agent login', can_test: true, hooks: { state: 'not_supported', mode: 'shadow', sentence: "This agent can't report to Puppet Master directly. Puppet Master reads its screen." } },
];

export function installTauriMock(scenario: HarnessScenario): void {
  const panes = scenario.panes ?? EXAMPLE_PANES;
  // In-memory plugin-store (one store, rid 1), keyed as the app writes it.
  // The two agent-authority switches, at the shipped defaults unless the scenario says otherwise.
  let authority = scenario.authority ?? { ask_before_agent_control: false, agents_may_merge: true };
  const store = new Map<string, unknown>([['settings', { theme: scenario.theme ?? 'dark' }]]);
  // Settings > Phone > Devices: one phone with full control in daily use, one watch-and-answer.
  const now = Math.floor(Date.now() / 1000);
  const day = 24 * 60 * 60;
  let pairedDevices = [
    { id: '6f1c2a90-aaaa-4bbb-8ccc-000000000001', name: "Maya's iPhone", public_key: 'pk1', paired_at: now - 40 * day, caps: ['watch', 'answer', 'type', 'control'], expires_at: now + 30 * day - 600, last_seen: now - 600, expired: false, idle_expiry_secs: 30 * day },
    { id: '6f1c2a90-aaaa-4bbb-8ccc-000000000002', name: 'Pixel 8', public_key: 'pk2', paired_at: now - 9 * day, caps: ['watch', 'answer'], expires_at: now + 21 * day, last_seen: now - 9 * day, expired: false, idle_expiry_secs: 30 * day },
    { id: '6f1c2a90-aaaa-4bbb-8ccc-000000000003', name: 'Old iPad', public_key: 'pk3', paired_at: now - 120 * day, caps: ['watch', 'answer', 'type', 'control'], expires_at: now - 2 * day, last_seen: now - 32 * day, expired: true, idle_expiry_secs: 30 * day },
  ];

  mockWindows(scenario.route === 'settings' ? 'settings' : scenario.route === 'askbar' ? 'askbar' : 'cockpit');
  mockIPC(
    (cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      const shipped = shipMock(scenario.ship, cmd, a);
      if (shipped.handled) return shipped.value;
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
          return standbyStatus(scenario.standbyIdleSecs, scenario.standbyBlocked);
        case 'agents_list':
          return agentsListing(scenario.headlessOnlyAgent);
        case 'pane_run_info':
          return scenario.runs?.[String(a.paneId)] ?? (a.paneId === PANE_CLAUDE ? EXAMPLE_RUN : null);
        case 'budget_for_pane':
          return scenario.budgets?.[String(a.paneId)] ?? null;
        case 'budget_adjust':
          return a.action === 'stop' ? 'Stopped at its budget.' : a.action === 'add_minutes' ? 'Added 30 min.' : 'Added 10 turns.';
        case 'pane_worktree':
          return a.paneId === PANE_CLAUDE ? EXAMPLE_WORKTREE : null;
        case 'second_opinion_agents':
          return EXAMPLE_REVIEWERS;
        case 'second_opinion_start':
          return EXAMPLE_REVIEW_START;
        case 'second_opinion_poll':
          return EXAMPLE_REVIEW_POLL;
        case 'second_opinion_cancel':
          return null;
        case 'second_opinion_send':
          return { route: 'run', target: 'refactor-auth' };
        case 'worktree_diff':
          return EXAMPLE_WORKTREE_DIFF;
        case 'pane_transcript':
          return a.paneId === PANE_CLAUDE ? EXAMPLE_TRANSCRIPT : null;
        case 'checkpoints_list':
          if (scenario.checkpointsState === 'too_big') {
            return { state: 'too_big', message: 'Undo is off for this folder: it is too big. Add big folders to .gitignore.', items: [] };
          }
          return a.paneId === PANE_CLAUDE ? EXAMPLE_CHECKPOINTS : { state: 'ok', message: '', items: [] };
        case 'checkpoint_diff':
          return EXAMPLE_FILE_CHANGES;
        case 'checkpoint_restore':
        case 'checkpoint_undo_last':
          return { safetyId: '7', written: 4, removed: 1, failed: [] };
        case 'turn_summaries':
          return a.paneId === PANE_CLAUDE ? EXAMPLE_TURN_SUMMARIES : [];
        case 'away_digest_pending':
          return scenario.awayDigest ? EXAMPLE_DIGEST : null;
        case 'away_digest_dismiss':
        case 'presence_cockpit_focus':
          return null;
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
        case 'canary_status':
          return scenario.canary ?? { agents: [], auto_check: true, notices: [], captures: 0 };
        case 'canary_set_auto':
          return { ...(scenario.canary ?? { agents: [], notices: [], captures: 0 }), auto_check: a.on };
        case 'canary_check':
          return (scenario.canary?.agents ?? []).find((x) => x.agent === a.agent) ?? null;
        case 'canary_dismiss_notice':
          return null;
        case 'canary_open_captures':
          return 'C:\Users\dev\AppData\Roaming\com.puppetmaster.app\captures';
        case 'agents_health':
          return { agents: DOCTOR_ROWS };
        case 'agents_install_plan': {
          const mode = scenario.doctorInstall?.plan ?? 'winget';
          const missing = DOCTOR_ROWS.find((r) => r.id === a.agentId) ?? DOCTOR_ROWS[2];
          const step = (index: number, label: string, tool: string | null, method: string, command: string, host: string, risk = false, note = '') => ({
            index, label, tool, method, command, source: `https://${host}/docs`, source_host: host, high_risk: risk, note,
          });
          if (mode === 'blocked') {
            return { agent_id: missing.id, label: missing.label, available: false, steps: [], hint: missing.install_hint, reason: 'This PC cannot install apps from the command line (winget is missing). Use the command below, or download it from the website.' };
          }
          const main =
            mode === 'script'
              ? step(0, missing.label, null, 'script', "irm 'https://opencode.ai/install.ps1' | iex", 'opencode.ai', true)
              : step(0, missing.label, null, mode === 'prereq' ? 'npm' : 'winget', mode === 'prereq' ? 'npm install -g opencode-ai' : 'winget install --id SST.opencode -e --source winget', 'opencode.ai', false, 'OpenCode runs best inside WSL, but the Windows version works.');
          const steps = mode === 'prereq'
            ? [step(0, 'Node.js', 'node', 'winget', 'winget install --id OpenJS.NodeJS.LTS -e --source winget', 'nodejs.org'), { ...main, index: 1 }]
            : [main];
          return { agent_id: missing.id, label: missing.label, available: true, reason: null, hint: missing.install_hint, steps };
        }
        case 'agents_install_run':
          return { pane_id: 'pane-install-1', command: a.shownCommand, label: 'OpenCode' };
        case 'agents_install_status': {
          const after = scenario.doctorInstall?.after ?? 'running';
          if (after === 'running') return { known: true, done: false, exit_code: null, message: null };
          if (after === 'cancelled') return { known: true, done: true, exit_code: 1223, message: 'The install was cancelled in the Windows prompt.' };
          return { known: true, done: true, exit_code: 0, message: null };
        }
        case 'agents_install_finish': {
          const after = scenario.doctorInstall?.after ?? 'running';
          const installed = { ...DOCTOR_ROWS[2], state: 'unchecked', installed: true, version: '1.18.33', reason: 'Installed. Press Test to check it can answer.', can_test: true };
          if (after === 'done_ok') return { path_added: ['C:\\Users\\dev\\AppData\\Local\\Microsoft\\WinGet\\Links'], health: installed, needs_restart: false };
          if (after === 'restart') return { path_added: [], health: DOCTOR_ROWS[2], needs_restart: true };
          return { path_added: [], health: DOCTOR_ROWS[2], needs_restart: false };
        }
        case 'agents_health_test':
          return { ...DOCTOR_ROWS[0], state: 'ready', reason: 'Ready. It answered the test question.' };
        case 'get_mcp_status':
          return {
            bridgeReachable: true,
            bridgeUrl: 'http://127.0.0.1:17321',
            bridgeVersion: '0.1.4',
            portFileExists: true,
            portFilePath: 'C:\Users\dev\.puppet-master\bridge.port',
            nodeAvailable: true,
            npmAvailable: true,
            npmPackageVersion: '0.1.4',
            launchCommand: 'npx -y @puppet-master/mcp',
            backends: [],
            overallReady: true,
            repairResults: [],
          };
        case 'install_npm_mcp_configs':
          return [
            { backend: 'claude_cli', installed: true, changed: true, message: 'Claude Code project MCP configured for puppet-master' },
            { backend: 'codex_cli', installed: true, changed: false, message: 'Codex project MCP already set' },
            { backend: 'opencode_cli', installed: true, changed: true, message: 'OpenCode project MCP configured for puppet-master' },
          ];
        case 'install_global_npm_mcp_configs':
          return [
            { backend: 'claude_cli_global', installed: true, changed: true, message: 'Claude Code user MCP configured for puppet-master' },
            { backend: 'codex_cli_global', installed: true, changed: true, message: 'Codex global MCP configured for puppet-master' },
            { backend: 'opencode_cli_global', installed: true, changed: false, message: 'OpenCode global MCP already set' },
          ];
        case 'list_agent_contexts':
          return [];
        case 'secret_storage_info':
          return { secure: true, label: 'Windows Credential Manager' };
        case 'secret_get_masked':
          return a.name === 'anthropic_api_key' ? { set: true, last4: 'WXYZ' } : { set: false, last4: null };
        case 'get_opencode_key_status':
          return { active_profile: 'a', profiles: [{ id: 'a', label: 'Primary', configured: true }, { id: 'b', label: 'Secondary', configured: false }], key_automation_mode: 'notify_only', restart_pane_on_rotate: true };
        case 'agent_authority_get':
          return authority;
        case 'agent_authority_set': {
          authority = {
            ask_before_agent_control: (a.askBeforeAgentControl as boolean | undefined) ?? authority.ask_before_agent_control,
            agents_may_merge: (a.agentsMayMerge as boolean | undefined) ?? authority.agents_may_merge,
          };
          return authority;
        }
        case 'audit_recent':
          return scenario.audit ?? [];
        case 'audit_verify':
          return { ok: true, entries: (scenario.audit ?? []).length, broken: null };
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
        case 'create_mobile_pairing_session': {
          const url = String(a.bridgeUrl ?? 'https://pm.example.com/bridge');
          return {
            pairing_code: 'K7Q2-M9XD',
            expires_at: 1_900_000_000,
            server_public_key: 'bW9jay1zZXJ2ZXItcHVibGljLWtleQ==',
            bridge_url: url,
            qr_payload: { v: 1, u: url, pk: 'bW9jay1zZXJ2ZXItcHVibGljLWtleQ==', c: 'K7Q2M9XD', e: 1_900_000_000 },
          };
        }
        case 'list_paired_mobile_devices':
          return pairedDevices;
        case 'set_paired_mobile_device_caps': {
          const device = pairedDevices.find((d) => d.id === a.deviceId);
          if (device) device.caps = (a.caps as string[]) ?? [];
          return Boolean(device);
        }
        case 'revoke_paired_mobile_device': {
          const before = pairedDevices.length;
          pairedDevices = pairedDevices.filter((d) => d.id !== a.deviceId);
          return pairedDevices.length !== before;
        }
        case 'get_mobile_tunnel_info':
          return { localUrl: 'http://127.0.0.1:1420', pwaUrl: 'http://127.0.0.1:1420', tunnelUrl: 'https://pm.example.com', bridgeProxyUrl: 'https://pm.example.com/bridge', updatedAt: 0 };
        case 'pane_set_title':
          return String(a.title ?? '') || null;
        case 'agent_pack_preview':
          return AGENT_PACK_PREVIEW;
        case 'agent_pack_apply':
          return { ...AGENT_PACK_PREVIEW, dry_run: false, recorded: 9 };
        case 'agent_pack_undo':
          return { restored: [], removed: ['a', 'b'], skipped: [], recorded: 0 };
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
        case 'team_state':
          return scenario.team ?? null;
        case 'team_config_get':
        case 'team_config_set':
        case 'team_create':
        case 'team_join':
        case 'team_leave':
        case 'team_join_approve':
        case 'team_join_decline':
        case 'team_member_remove':
        case 'team_member_confirm_github':
          return teamConfig({ enabled: !!scenario.team?.enabled, name: scenario.team?.me ?? 'Leo', ...scenario.teamConfig });
        case 'team_invite':
          return { link: 'pm://team/join?t=Acme&r=relay.acme.dev&k=8Vh2mQ0pXn4cD7wLrT1uYb3sZaE5gJfK', code: 'KQ7M-4XPD', expiresAtMs: Date.now() + 10 * 60_000 };
        case 'team_gaps_clear':
          return null;
        case 'team_merge_plan': {
          // What GitHub answers for the pull request (never the card's own words).
          const card = scenario.team?.cards.find((c) => c.id === a.cardId)?.event;
          const pr = card?.subject.pr;
          return {
            repo: 'acme/billing-service',
            number: pr?.number ?? 142,
            title: pr?.title ?? 'Invoice PDF export',
            author: card?.actor.display ?? 'priya-dev',
            headRef: card?.subject.branch ?? 'fix/rounding',
            headSha: 'e04bc0145a89bb85472ac53fa26cedc9f64a326f',
            baseRef: card?.subject.base ?? 'main',
            method: 'squash',
            methodWords: 'squashed into one commit',
            autoMergeOff: false,
          };
        }
        case 'team_merge_when_green': {
          const pr = scenario.team?.cards.find((c) => c.id === a.cardId)?.event.subject.pr;
          return `GitHub will merge #${pr?.number ?? 142} when its checks pass. Nothing was merged yet.`;
        }
        case 'team_job_diff':
          return TEAM_DIFF;
        case 'team_send':
        case 'team_draft_dismiss':
        case 'team_card_dismiss':
        case 'team_job_discard':
        case 'team_job_start':
          return null;
        case 'team_pull':
          return 'Pulled feat/auth. You are at 3f9c2a1.';
        case 'team_job_apply':
          return 'Applied to feat/billing. Nothing was pushed.';
        case 'memory_get':
        case 'memory_sync':
        case 'memory_save':
        case 'memory_set_project_enabled':
          return memoryView(typeof a.text === 'string' ? a.text : undefined);
        case 'memory_global_enabled':
          return true;
        case 'askbar_settings_get':
          return { enabled: true, shortcut: 'Ctrl+Alt+Space', default_agent: 'claude', last_agent: null, attach_clipboard_default: false };
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
        case 'askbar_resize': {
          // The real window is 640 wide and 64 or 420 tall; mimic it so screenshots look like the window.
          const root = document.getElementById('root');
          if (root) {
            root.style.width = '640px';
            root.style.height = a.expanded ? '420px' : '64px';
            root.style.margin = '48px auto 0';
          }
          return null;
        }
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
        case 'settings_patch': {
          // The Rust command in miniature: merge only the patched keys, null removes, ui_mode is not ours.
          const current = { ...((store.get('settings') as Record<string, unknown> | undefined) ?? {}) };
          for (const [key, value] of Object.entries((a.patch ?? {}) as Record<string, unknown>)) {
            if (key === 'ui_mode') continue;
            if (value === null) delete current[key];
            else current[key] = value;
          }
          store.set('settings', current);
          return current;
        }
        case 'diagnostic_preview':
          return {
            totalBytes: 41_530,
            files: [
              { path: 'README.txt', label: 'A short note on what this file is', bytes: 812 },
              { path: 'version.json', label: 'App and system versions', bytes: 148 },
              { path: 'settings.json', label: 'Your settings, with keys and tokens removed', bytes: 1_204 },
              { path: 'app-log.txt', label: "The last 400 lines of the app's log", bytes: 31_870 },
              { path: 'recordings/op-1.cast', label: 'Recording of a run that failed, 2 hours ago', bytes: 7_496 },
            ],
          };
        case 'diagnostic_show':
          return ['{', '  "app": "Puppet Master",', '  "version": "0.9.0",', '  "os": "windows"', '}'].join('\n');
        case 'diagnostic_save':
          return { path: String(a.path ?? ''), files: 6, bytes: 9_120 };
        case 'plugin:dialog|save':
          return 'C:\\Users\\dev\\Desktop\\puppet-master-diagnostic.zip';
        case 'get_app_install_info':
          return {
            version: '0.9.0',
            isPackaged: true,
            platform: 'windows',
            uninstallAvailable: true,
            uninstallInstructions: 'Removes Puppet Master with the Windows uninstaller.',
            dataDir: 'C:\\Users\\dev\\AppData\\Roaming\\com.puppetmaster.app',
          };
        case 'get_coordination_storage_info':
          return {
            scope: 'project',
            project_path: 'C:\\Users\\dev\\work\\puppet-master',
            storage_dir: 'C:\\Users\\dev\\work\\puppet-master\\.puppet-master',
            event_log_path: 'C:\\Users\\dev\\work\\puppet-master\\.puppet-master\\events.jsonl',
            event_count: 148,
            task_count: 3,
            lock_count: 1,
            exists: true,
          };
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

const MEMORY_TEXT = [
  '# puppet-master',
  '',
  'Desktop app: Tauri 2 (Rust) + React. Backend logic stays in Rust.',
  '',
  '- Run tests: cargo test --lib, npm test --workspace=@puppet-master/app',
  '- UI is monochrome, see mockups/ui-proposal/STYLE.md',
  '- Never push; commit small and revertable',
].join('\n');

function memoryView(text?: string) {
  return {
    project: 'C:\\Users\\dev\\work\\puppet-master',
    memory: text ?? MEMORY_TEXT,
    global_enabled: true,
    project_enabled: true,
    report: null,
    targets: [
      { path: 'CLAUDE.md', agents: ['claude'], status: 'in_sync', user_content: true },
      { path: 'AGENTS.md', agents: ['codex', 'opencode'], status: 'will_update', user_content: true },
      { path: '.cursor/rules/puppet-master.mdc', agents: ['cursor_agent'], status: 'will_create', user_content: false },
    ],
  };
}
