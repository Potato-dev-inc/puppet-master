// @vitest-environment jsdom
// Settings > Agents: the three rows render with their key controls, each control calls its own command,
// nothing here is destructive, and search finds each row by the words inside it.
import { unsearchableLabels } from '../test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { tauriAccountsApi } from '../../../lib/accounts';
import { tauriAgentDefinitionsApi } from '../../../lib/agentDefinitions';
import { DEFAULT_STANDBY_CONFIG } from '../../../lib/standby';
import type { KeepGoingSettings } from '../../../lib/keep-going';
import { tauri } from '../../../lib/tauri';
import type { StandbyConfig, StandbyStatus } from '../../../lib/tauri-types';
import { rowMatches, searchSections, type SectionDef } from '../model';
import { BASE_SETTINGS, blur, buttonByText, byLabel, click, hasText, mountRows, pickOption, settle, type as typeInto, type Mounted } from '../test-utils';
import { agentsRows } from './AgentsSection';

vi.mock('../../../lib/tauri', async (importOriginal) => {
  const real = await importOriginal<typeof import('../../../lib/tauri')>();
  return {
    ...real,
    tauri: { ...real.tauri, standbyGetStatus: vi.fn(async () => null), standbySetConfig: vi.fn(async () => null) },
  };
});

vi.mock('../../../lib/keep-going', async (importOriginal) => {
  const real = await importOriginal<typeof import('../../../lib/keep-going')>();
  return {
    ...real,
    keepGoingSettingsGet: vi.fn(async () => ({ order: [{ agent_id: 'fake_a' }], ask_back: true })),
    keepGoingSettingsSet: vi.fn(async (next: KeepGoingSettings) => next),
  };
});

vi.mock('../../../lib/accounts', async (importOriginal) => {
  const real = await importOriginal<typeof import('../../../lib/accounts')>();
  const overview = {
    agents: [
      { agent_id: 'claude', home_env: 'CLAUDE_CONFIG_DIR', login_hint: 'Run `claude` and log in, then close it.' },
      { agent_id: 'codex', home_env: 'CODEX_HOME', login_hint: 'Run `codex` and sign in, then close it.' },
    ],
    accounts: [{ id: 'acc_9', agent_id: 'claude', label: 'Work', home: 'C:/data/accounts/work' }],
  };
  return {
    ...real,
    tauriAccountsApi: {
      list: vi.fn(async () => overview),
      add: vi.fn(async (agentId: string, label: string) => ({ id: 'acc_10', agent_id: agentId, label, home: 'C:/data/accounts/new' })),
      rename: vi.fn(async (id: string, label: string) => ({ id, agent_id: 'claude', label, home: 'C:/data/accounts/work' })),
      remove: vi.fn(async () => false),
      openLoginPane: vi.fn(async () => 'pane-1'),
    },
  };
});

vi.mock('../../../lib/agent-health', async (importOriginal) => {
  const real = await importOriginal<typeof import('../../../lib/agent-health')>();
  const note = 'Puppet Master adds a small file to .codex in each project; Codex asks you to review it once per folder.';
  return {
    ...real,
    agentHealthApi: {
      check: vi.fn(async () => [
        { id: 'claude', label: 'Claude Code', state: 'unchecked', installed: true, version: '2.1.300', reason: '', install_hint: null, login_hint: null, can_test: true, hooks: { state: 'connected', mode: 'shadow', sentence: 'Connected.', opt_in: false, enabled: true } },
        { id: 'codex', label: 'Codex', state: 'unchecked', installed: true, version: '0.162.1', reason: '', install_hint: null, login_hint: null, can_test: true, hooks: { state: 'off', mode: 'off', sentence: 'Live updates are off.', opt_in: true, opt_in_note: note, enabled: false } },
      ]),
      test: vi.fn(),
    },
  };
});

vi.mock('../../../lib/agentDefinitions', async (importOriginal) => {
  const real = await importOriginal<typeof import('../../../lib/agentDefinitions')>();
  const listing = {
    sources: [
      { name: 'bundled', enabled: true, path: null, consent: null },
      { name: 'user', enabled: true, path: 'C:/data/agents', consent: null },
      { name: 'project', enabled: false, path: null, consent: false },
      { name: 'extra', enabled: false, path: null, consent: false },
    ],
    definitions: [{ id: 'kilo', file: 'C:/data/agents/kilo.toml', source: 'user', state: 'active', errors: [], fingerprint: 'sha256:abc', shadowed_by: null }],
    projects: [],
    extra_dirs: [],
  };
  const result = { ok: true, errors: [], rendered: { headless_argv: null, tui_argv: null } };
  return {
    ...real,
    tauriAgentDefinitionsApi: {
      list: vi.fn(async () => listing),
      reload: vi.fn(async () => ({ ...listing, diff: { added: [], removed: [], changed: [] } })),
      preview: vi.fn(async () => result),
      approve: vi.fn(async () => ({ ok: true })),
      revoke: vi.fn(async () => ({ ok: true })),
      consent: vi.fn(async () => ({ ok: true })),
      sourceSet: vi.fn(async () => ({ ok: true })),
      extraDir: vi.fn(async () => ({ ok: true })),
      projectPath: vi.fn(async () => ''),
      projectScan: vi.fn(async (project: string) => ({ project, files: [], consented: false, enabled: false })),
      addGeneric: vi.fn(async () => listing),
      exportDefinition: vi.fn(async (id: string) => ({ path: `C:/out/${id}.toml`, fixtures_zip: null })),
      importDefinition: vi.fn(async () => listing),
      checkFile: vi.fn(async () => ({ status: 'ran', dir: 'C:/data/agents/kilo', explicit: false, generic: false, reason: null, name: 'kilo', results: [] })),
      createFixturesDir: vi.fn(async () => 'C:/data/agents/kilo'),
      draftsList: vi.fn(async () => ({ dir: 'C:/data/agents/drafts', drafts: [] })),
      draftValidate: vi.fn(async () => result),
    },
  };
});

// A standby status as the backend would return it (same shape the PreloadSettings test uses).
function statusFor(config: StandbyConfig): StandbyStatus {
  return {
    enabled: config.preload.enabled,
    config,
    counts: { total: 0, ready: 0, starting: 0 },
    standbys: [],
    blocked: [],
    recent: [],
    stats: { spawned: 0, spawn_failed: 0, claimed: 0, missed: 0, discarded: 0, expired: 0, abandoned: 0 },
    supported: [],
  };
}

/** Preload reads and writes its own status through the standby commands; keep them in step. */
function useStandby(config: StandbyConfig = DEFAULT_STANDBY_CONFIG) {
  vi.mocked(tauri.standbyGetStatus).mockResolvedValue(statusFor(config));
  vi.mocked(tauri.standbySetConfig).mockImplementation(async (next: StandbyConfig) => statusFor(next));
}

let m: Mounted;

async function mount() {
  m = await mountRows(agentsRows);
  await settle(10);
  return m;
}

const switchFor = (label: string) => m.host.querySelector<HTMLButtonElement>(`[role="switch"][aria-label="${label}"]`);
const rowOf = (id: string) => m.host.querySelector<HTMLElement>(`[data-row="${id}"]`);

beforeEach(() => {
  vi.clearAllMocks();
  useStandby();
});

afterEach(async () => {
  await m?.unmount();
});

describe('Agents: every row renders', () => {
  it('shows the three rows, each with its key controls', async () => {
    await mount();
    for (const id of ['accounts', 'preload', 'versions', 'definitions']) expect(rowOf(id), `missing row: ${id}`).toBeTruthy();

    // Accounts
    expect(hasText(m.host, 'Accounts')).toBe(true);
    expect(byLabel(m.host, 'New claude account name')).toBeTruthy();
    expect(buttonByText(m.host, 'Add account')).toBeTruthy();
    expect(buttonByText(m.host, 'Open login pane')).toBeTruthy();
    expect(buttonByText(m.host, 'Remove')).toBeTruthy();

    // Preload
    expect(switchFor('Preload agents')).toBeTruthy();
    expect(byLabel(m.host, 'Keep Claude Code warm')).toBeTruthy();
    expect(byLabel(m.host, 'Preload idle timeout (minutes)')).toBeTruthy();
    expect(hasText(m.host, 'Agents to keep warm')).toBe(true);
    expect(hasText(m.host, 'Idle timeout')).toBe(true);
    expect(hasText(m.host, 'Status')).toBe(true);

    // Definitions
    expect(buttonByText(m.host, 'Reload')).toBeTruthy();
    expect(buttonByText(m.host, 'Import')).toBeTruthy();
    expect(byLabel(m.host, 'Command name')).toBeTruthy();
    expect(buttonByText(m.host, 'Add as generic agent')).toBeTruthy();
    expect(buttonByText(m.host, 'Refresh drafts')).toBeTruthy();
    expect(hasText(m.host, 'Sources')).toBe(true);
    expect(hasText(m.host, 'Consented projects')).toBe(true);
    expect(hasText(m.host, 'Extra directories')).toBe(true);
    expect(buttonByText(m.host, 'Export')).toBeTruthy();
    expect(buttonByText(m.host, 'Revoke')).toBeTruthy();
  });

  it('has no Planned badge and no Save button', async () => {
    await mount();
    expect(hasText(m.host, 'Planned')).toBe(false);
    expect(buttonByText(m.host, 'Save changes')).toBeUndefined();
  });

  it('groups the rows: Logins, Speed, Updates, Live updates, Keep going, then Definitions (whose own block carries the title)', async () => {
    await mount();
    const heads = [...m.host.querySelectorAll('section.st-group')].map((g) => g.querySelector(':scope > h2')?.textContent?.trim() ?? '');
    expect(heads).toEqual(['Logins', 'Speed', 'Updates', 'Live updates', 'Keep going', '']);
  });

  it('Keep going: the block mounts under its own row with the saved order', async () => {
    await mount();
    expect(rowOf('keep-going')).toBeTruthy();
    expect(hasText(m.host, 'When an agent runs out of usage')).toBe(true);
    expect(byLabel(m.host, 'Ask to move it back when the first agent resets')).toBeTruthy();
    expect(m.host.querySelector('ol[aria-label="Offer order"] > li')?.getAttribute('data-target-key')).toBe('fake_a');
  });
});

describe('Agents: each control saves through its own command', () => {
  it('Accounts: Add account creates the account for that agent only', async () => {
    await mount();
    const input = byLabel<HTMLInputElement>(m.host, 'New claude account name')!;
    await typeInto(input, 'Personal');
    await click(input.closest('form')!.querySelector<HTMLButtonElement>('button[type="submit"]'));
    await settle();
    expect(tauriAccountsApi.add).toHaveBeenCalledExactlyOnceWith('claude', 'Personal');
  });

  it('Accounts: Open login pane starts that agent under that account', async () => {
    await mount();
    await click(buttonByText(m.host, 'Open login pane'));
    await settle();
    expect(tauriAccountsApi.openLoginPane).toHaveBeenCalledExactlyOnceWith('claude', 'acc_9');
  });

  it('Preload: the master switch saves through the standby command, not settings.json', async () => {
    await mount();
    await click(switchFor('Preload agents'));
    await settle();
    expect(tauri.standbySetConfig).toHaveBeenLastCalledWith(expect.objectContaining({ preload: { enabled: true, providers: {} } }));
    expect(m.env.patches).toEqual([]);
  });

  it('Preload: ticking a provider keeps one of it warm', async () => {
    useStandby({ ...DEFAULT_STANDBY_CONFIG, preload: { enabled: true, providers: {} } });
    await mount();
    await click(byLabel(m.host, 'Keep Claude Code warm'));
    await settle();
    expect(tauri.standbySetConfig).toHaveBeenLastCalledWith(expect.objectContaining({ preload: { enabled: true, providers: { claude: 1 } } }));
    expect(m.env.patches).toEqual([]);
  });

  it('Preload: the idle timeout is typed in minutes and saved as seconds on blur', async () => {
    await mount();
    const idle = byLabel<HTMLInputElement>(m.host, 'Preload idle timeout (minutes)')!;
    await typeInto(idle, '30');
    await blur(idle);
    await settle();
    expect(tauri.standbySetConfig).toHaveBeenLastCalledWith(expect.objectContaining({ idle_timeout_secs: 1800 }));
  });

  it('Definitions: Reload and Import call their commands', async () => {
    await mount();
    await click(buttonByText(m.host, 'Reload'));
    await settle();
    expect(tauriAgentDefinitionsApi.reload).toHaveBeenCalledTimes(1);
    await click(buttonByText(m.host, 'Import'));
    await settle();
    expect(tauriAgentDefinitionsApi.importDefinition).toHaveBeenCalledTimes(1);
  });

  it('Definitions: Add as generic agent sends the typed command name', async () => {
    await mount();
    await typeInto(byLabel<HTMLInputElement>(m.host, 'Command name')!, 'mycoder');
    await click(buttonByText(m.host, 'Add as generic agent'));
    await settle();
    expect(tauriAgentDefinitionsApi.addGeneric).toHaveBeenCalledExactlyOnceWith('mycoder');
  });

  it('Definitions: Export and a source switch call their commands', async () => {
    await mount();
    await click(buttonByText(m.host, 'Export'));
    await settle();
    expect(tauriAgentDefinitionsApi.exportDefinition).toHaveBeenCalledExactlyOnceWith('kilo');
    await click(m.host.querySelector('[data-source="project"] [role="switch"]'));
    await settle();
    expect(tauriAgentDefinitionsApi.sourceSet).toHaveBeenCalledExactlyOnceWith('project', true);
  });

  it('Definitions: Revoke calls the revoke command for that definition', async () => {
    await mount();
    await click(buttonByText(m.host, 'Revoke'));
    await settle();
    expect(tauriAgentDefinitionsApi.revoke).toHaveBeenCalledExactlyOnceWith('kilo');
  });
});

describe('Agents: destructive actions', () => {
  it('no row is marked danger: Revoke and Remove are per-item actions inside the blocks', async () => {
    expect(agentsRows.every((row) => !row.danger)).toBe(true);
    await mount();
    expect(m.host.querySelector('[data-danger="true"]')).toBeNull();
  });
});

describe('Agents: Live updates', () => {
  const popup = () => byLabel<HTMLSelectElement>(m.host, 'Live updates')!;

  it('renders the row and shows Watch only by default', async () => {
    await mount();
    expect(rowOf('live-updates')).toBeTruthy();
    expect(popup().value).toBe('shadow');
    expect([...popup().options].map((o) => [o.value, o.textContent])).toEqual([
      ['shadow', 'Watch only'],
      ['off', 'Off'],
    ]);
    expect(hasText(m.host, 'Claude Code and Codex tell Puppet Master when they start, ask and finish.')).toBe(true);
  });

  it('choosing Off saves agent_hooks_mode off', async () => {
    await mount();
    await pickOption(popup(), 'off');
    expect(m.env.patch).toHaveBeenCalledWith({ agent_hooks_mode: 'off' });
  });

  it('a saved evidence mode still reads as Watch only', async () => {
    m = await mountRows(agentsRows, { settings: { ...BASE_SETTINGS, agent_hooks_mode: 'evidence' } });
    await settle(10);
    expect(popup().value).toBe('shadow');
  });

  it('never says hooks, shadow or evidence in its text', async () => {
    await mount();
    const text = rowOf('live-updates')!.textContent ?? '';
    expect(text).not.toMatch(/hook|shadow|evidence/i);
  });
});

describe('Agents: Live updates for opt-in agents', () => {
  const codexSwitch = () => byLabel<HTMLButtonElement>(m.host, 'Live updates from Codex');

  it('shows an off switch for an opt-in agent, with what turning it on does', async () => {
    await mount();
    await settle(10);
    expect(codexSwitch()).toBeTruthy();
    expect(codexSwitch()!.getAttribute('aria-checked')).toBe('false');
    expect(hasText(m.host, 'Puppet Master adds a small file to .codex in each project; Codex asks you to review it once per folder.')).toBe(true);
    expect(byLabel(m.host, 'Live updates from Claude Code')).toBeNull();
  });

  it('turning it on saves only that agent', async () => {
    await mount();
    await settle(10);
    await click(codexSwitch());
    expect(m.env.patch).toHaveBeenCalledWith({ agent_hooks_agents: { codex: 'shadow' } });
  });

  it('a saved choice wins over the default, and the global Off disables the switch', async () => {
    m = await mountRows(agentsRows, { settings: { ...BASE_SETTINGS, agent_hooks_agents: { codex: 'shadow' } } });
    await settle(10);
    expect(codexSwitch()!.getAttribute('aria-checked')).toBe('true');
    await click(codexSwitch());
    expect(m.env.patch).toHaveBeenCalledWith({ agent_hooks_agents: { codex: 'off' } });
    await m.unmount();
    m = await mountRows(agentsRows, { settings: { ...BASE_SETTINGS, agent_hooks_mode: 'off', agent_hooks_agents: { codex: 'shadow' } } });
    await settle(10);
    expect(codexSwitch()!.disabled).toBe(true);
  });
});

describe('Agents: search finds the rows', () => {
  const section: SectionDef = { id: 'agents', title: 'Agents', blurb: '', rows: agentsRows };
  const idsFor = (query: string) => searchSections([section], query).flatMap((hit) => hit.rows.map((row) => row.id));

  it('"idle timeout" finds Keep agents ready', () => {
    expect(idsFor('idle timeout')).toEqual(['preload']);
  });

  it('"revoke" finds Agent definitions', () => {
    expect(idsFor('revoke')).toEqual(['definitions']);
  });

  it('"work" finds Accounts', () => {
    expect(idsFor('work')).toEqual(['accounts']);
  });

  it('"keep going" finds Keep going, and "runs out" finds it too', () => {
    expect(idsFor('keep going')).toEqual(['keep-going']);
    expect(idsFor('runs out')).toEqual(['keep-going']);
  });

  it('search is case-insensitive and needs every word to match', () => {
    const preload = agentsRows.find((row) => row.id === 'preload')!;
    expect(rowMatches(preload, 'IDLE TIMEOUT')).toBe(true);
    expect(rowMatches(preload, 'idle revoke')).toBe(false);
  });
});

describe('Agents: search covers what the section shows', () => {
  it('every word of every label a row shows finds that row', async () => {
    await mount();
    expect(unsearchableLabels(m.host, agentsRows)).toEqual([]);
  });
});
