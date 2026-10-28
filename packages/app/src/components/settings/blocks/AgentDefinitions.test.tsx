// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { wizardResult, type AgentDefinitionsApi, type DefinitionsListing, type RemovedAgent } from '../../../lib/agentDefinitions';
import { AgentDefinitions } from './AgentDefinitions';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const KILO = 'C:/data/agents/kilo.toml';
const BAD = 'C:/data/agents/broken.toml';

function fixture(): DefinitionsListing {
  return {
    sources: [
      { name: 'bundled', enabled: true, path: null, consent: null },
      { name: 'user', enabled: true, path: 'C:/data/agents', consent: null },
      { name: 'project', enabled: false, path: null, consent: false },
      { name: 'extra', enabled: false, path: null, consent: false },
    ],
    definitions: [
      { id: 'kilo', file: KILO, source: 'user', state: 'needs_approval', errors: [], fingerprint: 'sha256:abc', shadowed_by: null },
      {
        id: null,
        file: BAD,
        source: 'user',
        state: 'invalid',
        errors: [{ code: 'AGENT_DEF_PARSE', message: 'expected a table', path: 'launch.argv', line: 7, column: 3 }],
        fingerprint: null,
        shadowed_by: null,
      },
    ],
  };
}

function mockApi(listing: DefinitionsListing = fixture()): AgentDefinitionsApi & Record<string, ReturnType<typeof vi.fn>> {
  return {
    list: vi.fn(async () => listing),
    reload: vi.fn(async () => ({ ...listing, diff: { added: ['kilo'], removed: [], changed: [] } })),
    preview: vi.fn(async () => ({
      ok: true,
      errors: [],
      rendered: { headless_argv: ['kilo', 'run', '--format', 'json', '<task>'], tui_argv: ['kilo', '--agent', 'build'] },
    })),
    approve: vi.fn(async () => ({ ok: true, id: 'kilo', state: 'active' })),
    revoke: vi.fn(async () => ({ ok: true })),
    consent: vi.fn(async () => ({ ok: true })),
    sourceSet: vi.fn(async () => ({ ok: true })),
    extraDir: vi.fn(async () => ({ ok: true })),
    projectPath: vi.fn(async () => ''),
    projectScan: vi.fn(async (project: string) => ({ project, files: [], consented: false, enabled: false })),
    addGeneric: vi.fn(async () => ({
      ...listing,
      definitions: [
        ...listing.definitions,
        { id: 'mycoder', file: 'C:/data/agents/mycoder.toml', source: 'user', state: 'needs_approval', errors: [], fingerprint: 'sha256:gen', shadowed_by: null },
      ],
    })),
    exportDefinition: vi.fn(async (id: string) => ({ path: `C:/out/${id}.toml`, fixtures_zip: null })),
    importDefinition: vi.fn(async () => ({
      ...listing,
      definitions: [
        ...listing.definitions,
        { id: 'shared', file: 'C:/data/agents/shared.toml', source: 'user', state: 'needs_approval', errors: [], fingerprint: 'sha256:imp', shadowed_by: null },
      ],
    })),
    checkFile: vi.fn(async () => ({
      status: 'ran',
      dir: 'C:/data/agents/kilo',
      explicit: false,
      generic: false,
      reason: null,
      name: 'kilo',
      results: [
        { fixture: 'headless_success.jsonl', passed: true, failures: [] },
        { fixture: 'tui_reply.txt', passed: false, failures: ['busy: expected true, got false'] },
      ],
    })),
    createFixturesDir: vi.fn(async () => 'C:/data/agents/kilo'),
  } as never;
}

let host: HTMLDivElement;
let root: Root;

async function render(api: AgentDefinitionsApi) {
  await act(async () => {
    root.render(<AgentDefinitions api={api} />);
  });
}

async function click(el: Element | null | undefined) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
}

const button = (label: string) =>
  Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.trim() === label);

beforeEach(() => {
  // These tests are about the flow as it is with "Finish without me" turned off (the switch is on by default).
  globalThis.localStorage.setItem('pm.agentWizard.finishWithoutMe', 'off');
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  globalThis.localStorage.removeItem('pm.agentWizard.finishWithoutMe');
  await act(async () => root.unmount());
  host.remove();
});

describe('AgentDefinitions', () => {
  it('renders source badge and state', async () => {
    await render(mockApi());
    const rows = host.querySelectorAll('[data-testid="definition-row"]');
    expect(rows).toHaveLength(2);
    expect(rows[0].textContent).toContain('kilo.toml');
    expect(rows[0].querySelector('[data-testid="source-badge"]')?.textContent).toBe('User');
    expect(rows[0].querySelector('[data-testid="state-badge"]')?.textContent).toBe('Needs approval');
    expect(rows[1].querySelector('[data-testid="state-badge"]')?.textContent).toBe('Invalid');
  });

  it('shows errors with code, file and line', async () => {
    await render(mockApi());
    const text = host.querySelectorAll('[data-testid="definition-row"]')[1].textContent ?? '';
    expect(text).toContain('AGENT_DEF_PARSE');
    expect(text).toContain('broken.toml:7:3');
    expect(text).toContain('expected a table');
  });

  it('reload button calls reload and shows the diff', async () => {
    const api = mockApi();
    await render(api);
    await click(button('Reload'));
    expect(api.reload).toHaveBeenCalledTimes(1);
    expect(host.textContent).toContain('added kilo');
  });

  it('approve dialog shows rendered argv and warnings', async () => {
    const api = mockApi();
    await render(api);
    await click(button('Review and approve'));
    expect(api.preview).toHaveBeenCalledWith(KILO);
    const dialog = host.querySelector('[role="dialog"]');
    expect(dialog?.textContent).toContain('kilo run --format json "<task>"');
    expect(dialog?.textContent).toContain('kilo --agent build');
    expect(dialog?.textContent).toContain('with your permissions');
    await click(button('Approve'));
    expect(api.approve).toHaveBeenCalledWith(KILO, 'sha256:abc');
    expect(host.querySelector('[role="dialog"]')).toBeNull();
  });

  it('approve dialog shows the full command: the program first, then the arguments', async () => {
    const api = mockApi();
    (api.preview as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      errors: [],
      rendered: {
        headless_argv: ['--output-format', 'json', '--model', '<model>', '--conversation', '<session>', '--print', '<task>'],
        tui_argv: ['--agent', 'build'],
      },
      program: { name: 'kilo', path: 'C:\\Users\\someone\\bin\\kilo.exe', display: '~\\bin\\kilo.exe', found: true, note: null, tui_argv_has_program: false },
    });
    await render(api);
    await click(button('Review and approve'));
    const dialog = host.querySelector('[role="dialog"]');
    expect(dialog?.textContent).toContain(
      '~\\bin\\kilo.exe --output-format json --model "<model>" --conversation "<session>" --print "<task>"',
    );
    expect(dialog?.textContent).toContain('~\\bin\\kilo.exe --agent build');
    // the user folder name never reaches the screen
    expect(dialog?.textContent).not.toContain('someone');
    expect(dialog?.textContent).not.toContain('not found on this PC');
  });

  it('approve dialog says so when the program is not installed', async () => {
    const api = mockApi();
    (api.preview as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      errors: [],
      rendered: { headless_argv: ['run', '<task>'], tui_argv: ['--agent', 'build'] },
      program: { name: 'kilo', path: null, display: 'kilo', found: false, note: 'not found on this PC', tui_argv_has_program: false },
    });
    await render(api);
    await click(button('Review and approve'));
    const dialog = host.querySelector('[role="dialog"]');
    expect(dialog?.textContent).toContain('kilo run "<task>"');
    expect(dialog?.textContent).toContain('kilo --agent build');
    expect(dialog?.textContent).toContain('kilo: not found on this PC');
  });

  it('approve shows a no-restart toast and never a restart prompt', async () => {
    const api = mockApi();
    await render(api);
    await click(button('Review and approve'));
    await click(button('Approve'));
    const toast = host.querySelector('[data-testid="agent-toast"]');
    expect(toast?.textContent).toMatch(/kilo is approved - no restart needed/);
    expect(host.textContent).not.toMatch(/restart Puppet Master/i);
  });

  it('shows an experimental badge on experimental rows only', async () => {
    const listing = fixture();
    listing.definitions[0] = { ...listing.definitions[0], experimental: true };
    await render(mockApi(listing));
    expect(host.querySelectorAll('[data-testid="experimental-badge"]')).toHaveLength(1);
  });

  it('lists recorder drafts and validates one through the api', async () => {
    const api = mockApi();
    api.draftsList = vi.fn(async () => ({
      dir: 'C:/data/agents/drafts',
      drafts: [{ name: 'kilo_draft.toml', path: 'C:/data/agents/drafts/kilo_draft.toml', modified_ms: 1, size: 10 }],
    }));
    api.draftValidate = vi.fn(async () => ({
      ok: false,
      errors: [{ code: 'AGENT_DEF_BAD_REGEX', message: 'bad', path: 'tui.busy', line: null, column: null }],
      rendered: { headless_argv: null, tui_argv: null },
    }));
    await render(api);
    const row = host.querySelector('[data-testid="draft-row"]');
    expect(row?.textContent).toContain('kilo_draft.toml');
    expect(host.textContent).toContain('C:/data/agents/drafts');
    await click(button('Validate draft'));
    expect(api.draftValidate).toHaveBeenCalledWith('C:/data/agents/drafts/kilo_draft.toml');
    expect(host.querySelector('[role="status"]')?.textContent).toContain('AGENT_DEF_BAD_REGEX');
  });

  it('escapes hostile label', async () => {
    const listing = fixture();
    listing.definitions[0].id = '<img src=x onerror="window.__pwned=1">';
    listing.definitions[0].file = 'C:/data/agents/<img src=x onerror=alert(1)>.toml';
    listing.definitions[1].errors[0].message = '<img src=x onerror=alert(2)>';
    await render(mockApi(listing));
    expect(host.querySelector('img')).toBeNull();
    expect(host.textContent).toContain('<img src=x onerror="window.__pwned=1">');
  });

  it('shows a load error instead of crashing', async () => {
    const api = mockApi();
    (api.list as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error('boom'));
    await render(api);
    expect(host.textContent).toContain('boom');
  });
});

describe('AgentDefinitions export/import buttons', () => {
  it('Export calls exportDefinition for a user definition and reports the path', async () => {
    const api = mockApi();
    await render(api);
    await click(button('Export'));
    expect(api.exportDefinition).toHaveBeenCalledTimes(1);
    expect(host.textContent).toContain('Exported');
    expect(host.textContent).toContain('.toml');
  });

  it('Import calls importDefinition, lists the new row unapproved and says it needs approval', async () => {
    const api = mockApi();
    await render(api);
    await click(button('Import'));
    expect(api.importDefinition).toHaveBeenCalledTimes(1);
    expect(host.textContent).toContain('shared');
    expect(host.textContent).toContain('unapproved');
  });

  it('a cancelled Import changes nothing', async () => {
    const api = mockApi();
    (api.importDefinition as ReturnType<typeof vi.fn>).mockResolvedValueOnce(null);
    await render(api);
    await click(button('Import'));
    expect(host.querySelectorAll('[data-testid="definition-row"]')).toHaveLength(2);
    expect(host.textContent).not.toContain('Imported.');
  });
});

describe('AgentDefinitions source toggle / extra dir / consent controls', () => {
  it('lists every source with its enabled state and path', async () => {
    await render(mockApi());
    const rows = host.querySelectorAll('[data-testid="source-row"]');
    expect(Array.from(rows).map((r) => r.getAttribute('data-source'))).toEqual(['bundled', 'user', 'project', 'extra']);
    expect(rows[1].textContent).toContain('C:/data/agents');
    expect(rows[0].textContent).toContain('Enabled');
  });

  it('source switch calls sourceSet and reloads the listing', async () => {
    const api = mockApi();
    await render(api);
    const sw = host.querySelector('[data-source="project"] [role="switch"]');
    expect(sw?.getAttribute('aria-checked')).toBe('false');
    await click(sw);
    expect(api.sourceSet).toHaveBeenCalledWith('project', true);
    expect(api.list).toHaveBeenCalledTimes(2);
  });

  it('shows the backend error when a source change is refused', async () => {
    const api = mockApi();
    (api.sourceSet as ReturnType<typeof vi.fn>).mockRejectedValueOnce('INVALID_ARGUMENT: nope');
    await render(api);
    await click(host.querySelector('[data-source="extra"] [role="switch"]'));
    expect(host.textContent).toContain('INVALID_ARGUMENT');
  });

  it('bundled has no switch', async () => {
    await render(mockApi());
    expect(host.querySelector('[data-source="bundled"] [role="switch"]')).toBeNull();
  });

  it('adds, consents and removes an extra directory', async () => {
    const listing = fixture();
    listing.extra_dirs = [{ path: 'D:/team', consent: false, exists: true }];
    const api = mockApi(listing);
    await render(api);
    const input = host.querySelector('input[aria-label="Extra directory"]') as HTMLInputElement;
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
      setter?.call(input, 'E:/more');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await click(button('Add directory'));
    expect(api.extraDir).toHaveBeenCalledWith('add', 'E:/more');
    const row = host.querySelector('[data-testid="extra-dir-row"]');
    expect(row?.textContent).toContain('D:/team');
    await click(row?.querySelector('[role="switch"]'));
    expect(api.consent).toHaveBeenCalledWith('extra', 'D:/team', true);
    await click(Array.from(row?.querySelectorAll('button') ?? []).find((b) => b.textContent === 'Remove'));
    expect(api.extraDir).toHaveBeenCalledWith('remove', 'D:/team');
  });

  it('lists consented projects and revokes consent', async () => {
    const listing = fixture();
    listing.projects = [{ path: 'c:/work/app', files: ['evil.toml'] }];
    const api = mockApi(listing);
    await render(api);
    const row = host.querySelector('[data-testid="project-consent-row"]');
    expect(row?.textContent).toContain('evil.toml');
    await click(button('Revoke consent'));
    expect(api.consent).toHaveBeenCalledWith('project', 'c:/work/app', false);
  });

  it('consent banner lists the project files and is not a modal', async () => {
    const api = mockApi();
    (api.projectPath as ReturnType<typeof vi.fn>).mockResolvedValue('C:/work/app');
    (api.projectScan as ReturnType<typeof vi.fn>).mockResolvedValue({
      project: 'c:/work/app',
      files: ['evil.toml', 'helper.toml'],
      consented: false,
      enabled: false,
    });
    await render(api);
    const banner = host.querySelector('[data-testid="consent-banner"]');
    expect(banner?.textContent).toContain('evil.toml');
    expect(banner?.textContent).toContain('helper.toml');
    expect(banner?.textContent).toContain('Nothing is loaded until you allow');
    expect(host.querySelector('[aria-modal="true"]')).toBeNull();
    await click(button('Allow for this project'));
    expect(api.consent).toHaveBeenCalledWith('project', 'c:/work/app', true);
  });

  it('consent banner can be dismissed and is absent for a consented or empty project', async () => {
    const api = mockApi();
    (api.projectPath as ReturnType<typeof vi.fn>).mockResolvedValue('C:/work/app');
    (api.projectScan as ReturnType<typeof vi.fn>).mockResolvedValue({ project: 'c:/work/app', files: ['a.toml'], consented: false, enabled: false });
    await render(api);
    await click(button('Not now'));
    expect(host.querySelector('[data-testid="consent-banner"]')).toBeNull();
    expect(api.consent).not.toHaveBeenCalled();

    for (const scan of [
      { project: 'c:/x', files: ['a.toml'], consented: true, enabled: true },
      { project: 'c:/x', files: [], consented: false, enabled: false },
    ]) {
      await act(async () => root.unmount());
      root = createRoot(host);
      (api.projectScan as ReturnType<typeof vi.fn>).mockResolvedValue(scan);
      await render(api);
      expect(host.querySelector('[data-testid="consent-banner"]')).toBeNull();
    }
  });

  it('AgentDefinitions shows a neutral message and a Create folder button when there are no fixtures', async () => {
    const api = mockApi();
    (api.checkFile as unknown as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce({
        status: 'no_fixtures',
        dir: 'C:/data/agents/kilo',
        explicit: false,
        generic: true,
        reason: 'missing',
        name: 'kilo',
        results: [],
      })
      .mockResolvedValueOnce({
        status: 'no_fixtures',
        dir: 'C:/data/agents/kilo',
        explicit: false,
        generic: true,
        reason: 'empty',
        name: 'kilo',
        results: [],
      });
    await render(api);
    await click(button('Check with fixtures'));
    const none = host.querySelector('[data-testid="fixtures-none"]')!;
    expect(none.textContent).toContain('No captures yet for kilo.');
    expect(none.textContent).toContain('C:/data/agents/kilo');
    expect(none.textContent).toContain('optional for experimental');
    expect(host.textContent).not.toContain('FAIL');
    expect(host.querySelector('[data-testid="fixture-result"]')).toBeNull();
    await click(button('Create folder'));
    expect(api.createFixturesDir).toHaveBeenCalledWith(KILO, undefined);
    expect(host.querySelector('[data-testid="fixtures-none"]')!.textContent).toContain('exists but has no captures');
  });

  it('AgentDefinitions explains an explicit fixtures folder that does not exist', async () => {
    const api = mockApi();
    (api.checkFile as unknown as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'no_fixtures',
      dir: 'D:/nope',
      explicit: true,
      generic: false,
      reason: 'missing',
      name: 'kilo',
      results: [],
    });
    await render(api);
    await click(button('Check with fixtures'));
    const text = host.querySelector('[data-testid="fixtures-none"]')!.textContent!;
    expect(text).toContain('does not exist: D:/nope');
    expect(text).not.toContain('os error');
  });

  it('AgentDefinitions check button shows per-fixture results', async () => {
    const api = mockApi();
    await render(api);
    await click(button('Check with fixtures'));
    expect(api.checkFile).toHaveBeenCalledWith(KILO, undefined);
    const rows = Array.from(host.querySelectorAll('[data-testid="fixture-result"]'));
    expect(rows).toHaveLength(2);
    expect(rows[0].textContent).toContain('PASS');
    expect(rows[0].textContent).toContain('headless_success.jsonl');
    expect(rows[1].textContent).toContain('FAIL');
    expect(rows[1].textContent).toContain('busy: expected true, got false');
  });

  it('AgentDefinitions add generic creates unapproved file', async () => {
    const api = mockApi();
    await render(api);
    const input = host.querySelector('input[aria-label="Command name"]') as HTMLInputElement;
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
      setter.call(input, 'mycoder');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await click(button('Add as generic agent'));
    expect(api.addGeneric).toHaveBeenCalledWith('mycoder');
    const rows = Array.from(host.querySelectorAll('[data-testid="definition-row"]'));
    const added = rows.find((r) => r.textContent?.includes('mycoder'));
    expect(added?.querySelector('[data-testid="state-badge"]')?.textContent).toBe('Needs approval');
    expect(api.approve).not.toHaveBeenCalled();
  });
});

describe('AgentDefinitions approval hardening', () => {
  it('AgentDefinitions diff view and warnings', async () => {
    const listing = fixture();
    listing.definitions[0].state = 'approval_stale';
    const api = mockApi(listing);
    (api.preview as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      errors: [],
      rendered: { headless_argv: ['run', '--yolo', '<task>'], tui_argv: null },
      warnings: ['The command passes `--yolo`, which usually skips every confirmation.'],
      approved: {
        headless_argv: ['run', '<task>'],
        tui_argv: null,
        binary: 'C:/bin/kilo.exe',
        approved_at: '2026-10-09T00:00:00Z',
        fingerprint: 'sha256:old',
      },
    });
    await render(api);
    expect(host.querySelector('[data-testid="state-badge"]')?.textContent).toBe('Changed since approval');
    await click(button('Review and approve'));
    const dialog = host.querySelector('[role="dialog"]');
    const diff = dialog?.querySelector('[data-testid="approval-diff"]');
    expect(diff?.textContent).toContain('Changed since you approved it');
    expect(diff?.querySelector('[data-diff="added"]')?.textContent).toContain('--yolo');
    expect(diff?.querySelector('[data-diff="removed"]')).toBeNull();
    expect(dialog?.textContent).toContain('--yolo');
    expect(dialog?.textContent).toContain('skips every confirmation');
  });

  it('an active definition whose binary moved asks for re-approval', async () => {
    const listing = fixture();
    listing.definitions[0].state = 'active';
    listing.definitions[0].binary_changed = true;
    const api = mockApi(listing);
    (api.preview as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      errors: [],
      rendered: { headless_argv: ['run'], tui_argv: null },
      binary_changed: true,
      approved: { headless_argv: ['run'], tui_argv: null, binary: 'C:/old/kilo.exe', approved_at: 'x', fingerprint: 'sha256:abc' },
    });
    await render(api);
    await click(button('Review and approve'));
    const dialog = host.querySelector('[role="dialog"]');
    expect(dialog?.textContent).toContain('resolves to a different file');
    expect(dialog?.textContent).toContain('C:/old/kilo.exe');
    await click(button('Approve'));
    expect(api.approve).toHaveBeenCalledWith(KILO, 'sha256:abc');
  });
});

describe('AgentDefinitions: Browse more agents', () => {
  it('has no Browse more agents button when the host gives no onOpenStore', async () => {
    await render(mockApi());
    expect(button('Browse more agents')).toBeUndefined();
    expect(button('Add with wizard')).toBeTruthy();
  });

  it('shows Browse more agents next to Add with wizard and calls onOpenStore when clicked', async () => {
    const onOpenStore = vi.fn();
    await act(async () => {
      root.render(<AgentDefinitions api={mockApi()} onOpenStore={onOpenStore} />);
    });
    const browse = button('Browse more agents');
    expect(browse).toBeTruthy();
    // same row as "Add with wizard", right after it
    expect(browse?.previousElementSibling).toBe(button('Add with wizard'));
    expect(onOpenStore).not.toHaveBeenCalled();
    await click(browse);
    expect(onOpenStore).toHaveBeenCalledTimes(1);
  });
});

describe('AgentDefinitions: Add with wizard', () => {
  const F = '`'.repeat(3);
  const WIZ = 'C:/data/agents/fake_agent.toml';

  function wizardMock() {
    const written = {
      id: 'fake_agent',
      file: WIZ,
      source: 'user',
      state: 'needs_approval',
      errors: [],
      fingerprint: 'sha256:wiz',
      shadowed_by: null,
    };
    const listing = { ...fixture(), definitions: [...fixture().definitions, written] };
    return {
      listing,
      api: {
        start: vi.fn(async () => ({
          session: 'wz_1',
          state: 'resolved',
          command: 'fake-agent',
          id_suggestion: 'fake_agent',
          program: { path: 'C:/x/fake-agent.exe', display: '~/fake-agent.exe', kind: 'native' },
          version: { ok: true, args: ['--version'], text: 'fake-agent 1.4.2', number: '1.4.2' },
          native_candidates: [],
          cost_note: 'The live test runs fake-agent 3 times.',
          limits: {},
        })),
        probe: vi.fn(async () => ({
          state: 'probed',
          runs: [{ label: '--help', args: ['--help'], text: 'Usage: fake-agent <COMMAND>\n  run  Run one task non-interactively', exit: 0, timed_out: false, truncated: false, bytes: 27, duplicate_of: null }],
          subcommands: [],
          skipped: [],
        })),
        verify: vi.fn(async () => ({
          ok: true,
          state: 'verified',
          errors: [],
          text: '',
          warnings: [],
          rendered: { headless_argv: ['run', '--', '<task>'], resume_argv: null },
          checked: { fixtures: 1, rules: 8 },
          samples_replayed: 0,
          planned: {
            program: { path: 'C:/x/fake-agent.exe', display: '~/fake-agent.exe' },
            stages: [{ name: 'pong', label: 'Say pong', args: ['run', '--', '<test message>'] }],
            stdin: null,
            env: [],
            env_unset: [],
            fingerprint: 'plan-A',
          },
        })),
        livetest: vi.fn(async () => ({ ok: true, state: 'tested', stages: [], errors: [], text: '', tested_version: '1.4.2', fixtures: [] })),
        finish: vi.fn(async () => ({
          ok: true,
          id: 'fake_agent',
          file: WIZ,
          fixtures_dir: 'C:/data/agents/fake_agent',
          provenance_file: 'C:/data/agents/fake_agent.wizard.json',
          state: 'needs_approval',
          fixtures: { passed: 1, total: 1 },
          listing,
        })),
        cancel: vi.fn(async () => ({ ok: true })),
        ask: vi.fn(async () => `${F}toml
schema_version = 1
${F}`),
        drafters: vi.fn(async () => [{ id: 'claude', label: 'Claude Code' }]),
      },
    };
  }

  it('has an Add with wizard button that opens the wizard and Cancel closes it', async () => {
    const api = mockApi();
    const { api: wizardApi } = wizardMock();
    await act(async () => {
      root.render(<AgentDefinitions api={api} wizardApi={wizardApi as never} />);
    });
    expect(host.querySelector('[aria-label="Add an agent with the wizard"]')).toBeNull();
    await click(button('Add with wizard'));
    expect(host.querySelector('[aria-label="Add an agent with the wizard"]')).toBeTruthy();
    expect(host.textContent).toContain('Which command starts it?');
  });

  it('hands the written definition to the normal approval dialog', async () => {
    const api = mockApi();
    (api.preview as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      errors: [],
      rendered: { headless_argv: ['run', '--', '<task>'], tui_argv: null },
    });
    const { api: wizardApi } = wizardMock();
    await act(async () => {
      root.render(<AgentDefinitions api={api} wizardApi={wizardApi as never} />);
    });
    await click(button('Add with wizard'));
    const input = host.querySelector('#wizard-command') as HTMLInputElement;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set?.call(input, 'fake-agent');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await click(button('Find it'));
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });
    await click(button('Skip the visible part'));
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });
    await click(button('Start'));
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });
    await click(button('Run the live test'));
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });
    await click(button('Approve'));
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });
    // the wizard is gone; the approval dialog for the new definition is open, with the exact command
    expect(host.querySelector('[aria-label="Add an agent with the wizard"]')).toBeNull();
    expect(api.preview).toHaveBeenCalledWith(WIZ);
    const dialog = host.querySelector('[role="dialog"]');
    expect(dialog?.getAttribute('aria-label')).toBe('Approve fake_agent');
    expect(dialog?.textContent).toContain('run -- "<task>"');
    expect(api.approve).not.toHaveBeenCalled();
    expect(host.querySelector('[data-testid="agent-toast"]')?.textContent).toContain('The wizard wrote fake_agent.');
    await click(button('Approve'));
    expect(api.approve).toHaveBeenCalledWith(WIZ, 'sha256:wiz');
  });
});

describe('AgentDefinitions: Remove, Undo and Recently removed', () => {
  const PROJECT_FILE = 'D:/repo/.puppet-master/agents/teamy.toml';
  const EXTRA_FILE = 'E:/team-agents/shared.toml';

  function listingWithOthers(): DefinitionsListing {
    const listing = fixture();
    listing.definitions[0] = { ...listing.definitions[0], label: 'Kilo Code' };
    listing.definitions.push(
      { id: 'teamy', file: PROJECT_FILE, source: 'project', state: 'active', errors: [], fingerprint: 'sha256:p', shadowed_by: null },
      { id: 'shared', file: EXTRA_FILE, source: 'extra', state: 'needs_approval', errors: [], fingerprint: 'sha256:e', shadowed_by: null },
    );
    return listing;
  }

  function removeApi(removed: RemovedAgent[] = []) {
    const base = mockApi(listingWithOthers());
    const extra = {
      remove: vi.fn(async () => ({
        ok: true, id: 'kilo', label: 'Kilo Code', removed_id: 'kilo-20261016-123456', fixtures: true, wizard: false, was_approved: false,
      })),
      restore: vi.fn(async () => ({ ok: true, id: 'kilo', label: 'Kilo Code', approval_restored: false, needs_approval: true })),
      removedList: vi.fn(async () => ({ removed })),
    };
    return Object.assign(base, extra);
  }

  const removeButton = (row: Element) => Array.from(row.querySelectorAll('button')).find((b) => b.textContent === 'Remove');
  const dialogButton = (label: string) =>
    Array.from(host.querySelectorAll('[role="dialog"] button')).find((b) => b.textContent === label);

  it('shows Remove on the rows of your own agents folder, also an invalid one, and nowhere else', async () => {
    await render(removeApi());
    const rows = host.querySelectorAll('[data-testid="definition-row"]');
    expect(removeButton(rows[0])).toBeTruthy();
    expect(removeButton(rows[1])).toBeTruthy();
    expect(removeButton(rows[2])).toBeUndefined();
    expect(removeButton(rows[3])).toBeUndefined();
  });

  it('a project or extra folder row says where it comes from instead of offering Remove', async () => {
    await render(removeApi());
    const rows = host.querySelectorAll('[data-testid="definition-row"]');
    expect(rows[2].querySelector('[data-testid="comes-from"]')?.textContent).toBe(
      'This agent comes from D:/repo/.puppet-master/agents. Remove it there, or turn it off with Revoke.',
    );
    expect(rows[3].querySelector('[data-testid="comes-from"]')?.textContent).toContain('This agent comes from E:/team-agents.');
    expect(rows[0].querySelector('[data-testid="comes-from"]')).toBeNull();
  });

  it('has no Remove at all when the backend cannot remove', async () => {
    await render(mockApi(listingWithOthers()));
    expect(Array.from(host.querySelectorAll('[data-testid="definition-row"]')).some((r) => removeButton(r))).toBe(false);
    expect(host.querySelector('[data-testid="comes-from"]')).toBeNull();
  });

  it('the confirm sheet says what moves, that running panes keep running, and that it can be undone', async () => {
    const api = removeApi();
    await render(api);
    await click(removeButton(host.querySelectorAll('[data-testid="definition-row"]')[0]));
    const dialog = host.querySelector('[role="dialog"]');
    expect(dialog?.getAttribute('aria-label')).toBe('Remove Kilo Code');
    const text = dialog?.textContent ?? '';
    expect(text).toContain('Remove Kilo Code?');
    expect(text).toContain('Nothing is deleted');
    expect(text).toContain('kilo.toml');
    expect(text).toContain('kilo/');
    expect(text).toContain('kilo.wizard.json');
    expect(text).toContain('Its approval');
    expect(text).toContain('Panes already running this agent keep running');
    expect(text).toContain('undo');
    expect(api.remove).not.toHaveBeenCalled();
    await click(dialogButton('Cancel'));
    expect(host.querySelector('[role="dialog"]')).toBeNull();
    expect(api.remove).not.toHaveBeenCalled();
  });

  it('Remove moves the agent, refreshes the list and offers Undo; Undo brings it back', async () => {
    const api = removeApi();
    await render(api);
    await click(removeButton(host.querySelectorAll('[data-testid="definition-row"]')[0]));
    const listCalls = (api.list as ReturnType<typeof vi.fn>).mock.calls.length;
    await click(dialogButton('Remove'));
    expect(api.remove).toHaveBeenCalledExactlyOnceWith(KILO);
    expect((api.list as ReturnType<typeof vi.fn>).mock.calls.length).toBeGreaterThan(listCalls);
    expect(host.querySelector('[role="dialog"]')).toBeNull();
    const toast = host.querySelector('[data-testid="undo-toast"]');
    expect(toast?.textContent).toContain('Removed Kilo Code.');
    await click(Array.from(toast?.querySelectorAll('button') ?? []).find((b) => b.textContent === 'Undo'));
    expect(api.restore).toHaveBeenCalledExactlyOnceWith('kilo-20261016-123456');
    expect(host.querySelector('[data-testid="undo-toast"]')).toBeNull();
    expect(host.querySelector('[data-testid="agent-toast"]')?.textContent).toBe(
      'Restored Kilo Code. Review and approve it again before it can run.',
    );
  });

  it('a refused Undo says so plainly, without the error code', async () => {
    const api = removeApi();
    api.restore.mockRejectedValueOnce(
      'AGENT_DEF_RESTORE_CLASH: A file named kilo.toml is already in your agents folder. Rename or move it, then restore again.',
    );
    await render(api);
    await click(removeButton(host.querySelectorAll('[data-testid="definition-row"]')[0]));
    await click(dialogButton('Remove'));
    await click(Array.from(host.querySelectorAll('[data-testid="undo-toast"] button')).find((b) => b.textContent === 'Undo'));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe(
      'A file named kilo.toml is already in your agents folder. Rename or move it, then restore again.',
    );
  });

  it('a failed Remove shows the plain message and no Undo', async () => {
    const api = removeApi();
    api.remove.mockRejectedValueOnce('AGENT_DEF_REMOVE_FAILED: could not move kilo: access denied');
    await render(api);
    await click(removeButton(host.querySelectorAll('[data-testid="definition-row"]')[0]));
    await click(dialogButton('Remove'));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('could not move kilo: access denied');
    expect(host.querySelector('[data-testid="undo-toast"]')).toBeNull();
    expect(host.querySelector('[role="dialog"]')).toBeNull();
  });

  it('lists Recently removed and restores one with Restore', async () => {
    const api = removeApi([
      { removed_id: 'old-20261015-101010', id: 'old', label: 'Old Agent', file_name: 'old.toml', removed_at: '2026-10-15T10:10:10Z', fixtures: false, wizard: false, was_approved: true },
    ]);
    api.restore.mockResolvedValueOnce({ ok: true, id: 'old', label: 'Old Agent', approval_restored: true, needs_approval: false } as never);
    await render(api);
    const rows = host.querySelectorAll('[data-testid="removed-row"]');
    expect(rows).toHaveLength(1);
    expect(rows[0].textContent).toContain('Old Agent');
    expect(rows[0].textContent).toContain('old.toml');
    expect(host.textContent).toContain('Recently removed');
    await click(rows[0].querySelector('button'));
    expect(api.restore).toHaveBeenCalledExactlyOnceWith('old-20261015-101010');
    expect(host.querySelector('[data-testid="agent-toast"]')?.textContent).toBe('Restored Old Agent. It is active again.');
    expect(api.removedList.mock.calls.length).toBeGreaterThan(1);
  });

  it('hides Recently removed while nothing was removed', async () => {
    await render(removeApi([]));
    expect(host.textContent).not.toContain('Recently removed');
  });
});

describe('AgentDefinitions: Clear all and Dismiss (Delete for good)', () => {
  const KILO_REMOVED: RemovedAgent = {
    removed_id: 'kilo-20261015-101010', id: 'kilo', label: 'Kilo Code', file_name: 'kilo.toml', removed_at: '2026-10-15T10:10:10Z',
    fixtures: false, wizard: false, was_approved: false,
  };
  const OLD_REMOVED: RemovedAgent = {
    removed_id: 'old-20261014-101010', id: 'old', label: 'Old Agent', file_name: 'old.toml', removed_at: '2026-10-14T10:10:10Z',
    fixtures: false, wizard: false, was_approved: true,
  };

  // The removed list is stateful, so a refresh after a purge shows what is left.
  function purgeApi(removed: RemovedAgent[] = [KILO_REMOVED, OLD_REMOVED], total?: number) {
    let current = removed;
    return Object.assign(mockApi(fixture()), {
      removedList: vi.fn(async () => ({ removed: current, total: total ?? current.length })),
      restore: vi.fn(),
      purge: vi.fn(async (removedId: string) => {
        current = current.filter((r) => r.removed_id !== removedId);
        return { ok: true, removed_id: removedId, deleted: 1 };
      }),
      purgeAll: vi.fn(async () => {
        const deleted = current.length;
        current = [];
        return { ok: true, deleted, failed: 0 };
      }),
    });
  }

  const dialogButton = (label: string) =>
    Array.from(host.querySelectorAll('[role="dialog"] button')).find((b) => b.textContent === label);
  const dismissButton = (label: string) => host.querySelector(`button[aria-label="Dismiss ${label}"]`);
  const rows = () => host.querySelectorAll('[data-testid="removed-row"]');

  it('shows Clear all in the header and Dismiss on each row, right after Restore', async () => {
    await render(purgeApi());
    expect(button('Clear all')).toBeTruthy();
    expect(rows()).toHaveLength(2);
    expect(Array.from(rows()[0].querySelectorAll('button')).map((b) => b.textContent)).toEqual(['Restore', 'Dismiss']);
    expect(dismissButton('Kilo Code')).toBeTruthy();
  });

  it('Dismiss asks first, and Cancel changes nothing', async () => {
    const api = purgeApi();
    await render(api);
    await click(dismissButton('Kilo Code'));
    expect(host.querySelector('[role="dialog"]')?.textContent).toContain("Delete Kilo Code for good? This can't be undone.");
    expect(api.purge).not.toHaveBeenCalled();
    await click(dialogButton('Cancel'));
    expect(host.querySelector('[role="dialog"]')).toBeNull();
    expect(api.purge).not.toHaveBeenCalled();
    expect(api.purgeAll).not.toHaveBeenCalled();
    expect(rows()).toHaveLength(2);
  });

  it('Dismiss deletes that one entry for good, refreshes the list and leaves the others', async () => {
    const api = purgeApi();
    await render(api);
    const listCalls = api.removedList.mock.calls.length;
    await click(dismissButton('Kilo Code'));
    await click(dialogButton('Delete for good'));
    expect(api.purge).toHaveBeenCalledExactlyOnceWith('kilo-20261015-101010');
    expect(api.purgeAll).not.toHaveBeenCalled();
    expect(api.removedList.mock.calls.length).toBeGreaterThan(listCalls);
    expect(host.querySelector('[role="dialog"]')).toBeNull();
    expect(rows()).toHaveLength(1);
    expect(rows()[0].textContent).toContain('Old Agent');
    expect(host.querySelector('[data-testid="agent-toast"]')?.textContent).toBe('Deleted Kilo Code for good.');
  });

  it('a refused Dismiss says so plainly and keeps the row', async () => {
    const api = purgeApi();
    api.purge.mockRejectedValueOnce('AGENT_DEF_PURGE_FAILED: could not delete the removed agent: access denied');
    await render(api);
    await click(dismissButton('Kilo Code'));
    await click(dialogButton('Delete for good'));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('could not delete the removed agent: access denied');
    expect(host.querySelector('[role="dialog"]')).toBeNull();
    expect(rows()).toHaveLength(2);
  });

  it('Clear all asks first with the full count, Cancel changes nothing, then it deletes everything and hides the block', async () => {
    const api = purgeApi([KILO_REMOVED, OLD_REMOVED], 60);
    await render(api);
    await click(button('Clear all'));
    expect(host.querySelector('[role="dialog"]')?.textContent).toContain("Delete 60 removed agents for good? This can't be undone.");
    expect(api.purgeAll).not.toHaveBeenCalled();
    await click(dialogButton('Cancel'));
    expect(host.querySelector('[role="dialog"]')).toBeNull();
    expect(api.purgeAll).not.toHaveBeenCalled();
    expect(rows()).toHaveLength(2);

    await click(button('Clear all'));
    await click(dialogButton('Delete for good'));
    expect(api.purgeAll).toHaveBeenCalledOnce();
    expect(api.purge).not.toHaveBeenCalled();
    expect(host.querySelector('[role="dialog"]')).toBeNull();
    expect(rows()).toHaveLength(0);
    expect(host.textContent).not.toContain('Recently removed');
    expect(host.querySelector('[data-testid="agent-toast"]')?.textContent).toBe('Deleted 2 removed agents for good.');
  });

  it('Clear all reports the entries it could not delete', async () => {
    const api = purgeApi();
    api.purgeAll.mockResolvedValueOnce({ ok: false, deleted: 1, failed: 1 } as never);
    await render(api);
    await click(button('Clear all'));
    await click(dialogButton('Delete for good'));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('1 could not be deleted. Try again.');
  });
});

describe('AgentDefinitions: approved by the wizard', () => {
  const rows = (): DefinitionsListing => ({
    ...fixture(),
    definitions: [
      { id: 'nimbus', file: 'C:/data/agents/nimbus.toml', source: 'user', state: 'active', errors: [], fingerprint: 'sha256:n', shadowed_by: null, approved_by: 'wizard', approval_checks: 9 },
      { id: 'kilo', file: KILO, source: 'user', state: 'active', errors: [], fingerprint: 'sha256:k', shadowed_by: null, approved_by: null, approval_checks: null },
    ],
  });

  it('says on the agent the wizard approved that it did, after how many checks, and keeps Revoke', async () => {
    await render(mockApi(rows()));
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });
    const items = Array.from(host.querySelectorAll('[data-testid="definition-row"]'));
    const nimbus = items.find((i) => i.textContent?.includes('nimbus'));
    const kilo = items.find((i) => i.textContent?.includes('kilo'));
    expect(nimbus?.querySelector('[data-testid="approved-by-wizard"]')?.textContent).toBe('Approved by the wizard after 9 checks passed');
    expect(Array.from(nimbus?.querySelectorAll('button') ?? []).map((b) => b.textContent)).toEqual(expect.arrayContaining(['Revoke']));
    expect(kilo?.querySelector('[data-testid="approved-by-wizard"]')).toBeNull();
  });

  it('says on an agent added from the Agent Store that it came from the store, and not on the others', async () => {
    const listing = rows();
    listing.definitions[1] = { ...listing.definitions[1], approved_by: 'store', approval_checks: 7 };
    await render(mockApi(listing));
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });
    const items = Array.from(host.querySelectorAll('[data-testid="definition-row"]'));
    const nimbus = items.find((i) => i.textContent?.includes('nimbus'));
    const kilo = items.find((i) => i.textContent?.includes('kilo'));
    expect(kilo?.querySelector('[data-testid="approved-by-store"]')?.textContent).toBe('Added from the Agent Store');
    expect(kilo?.textContent).not.toContain('Approved by the wizard');
    expect(nimbus?.querySelector('[data-testid="approved-by-store"]')).toBeNull();
  });

  it('opens no approval dialog for what the wizard approved, and says what it left for the person otherwise', () => {
    const done = { id: 'nimbus' };
    expect(wizardResult({ ...done, approval: { approved: true, count: 9 } })).toEqual({
      note: 'The wizard wrote nimbus and approved it after 9 checks passed. You can revoke or remove it below.',
      openReview: false,
    });
    expect(wizardResult({ ...done, approval: { approved: false, reason: 'the checker still reports a problem with this draft' } })).toEqual({
      note: 'The wizard wrote nimbus but did not approve it (the checker still reports a problem with this draft). Check the command below, then approve it.',
      openReview: true,
    });
    expect(wizardResult(done)).toEqual({ note: 'The wizard wrote nimbus. Check the command below, then approve it.', openReview: true });
  });
});
