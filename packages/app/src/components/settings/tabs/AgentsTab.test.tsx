// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentDefinitionsApi, DefinitionsListing } from '../../../lib/agentDefinitions';
import { AgentsTab } from './AgentsTab';

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
    checkFile: vi.fn(async () => [
      { fixture: 'headless_success.jsonl', passed: true, failures: [] },
      { fixture: 'tui_reply.txt', passed: false, failures: ['busy: expected true, got false'] },
    ]),
  } as never;
}

let host: HTMLDivElement;
let root: Root;

async function render(api: AgentDefinitionsApi) {
  await act(async () => {
    root.render(<AgentsTab api={api} />);
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
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

describe('AgentsTab', () => {
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

describe('AgentsTab export/import buttons', () => {
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

describe('AgentsTab source toggle / extra dir / consent controls', () => {
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

  it('AgentsTab check button shows per-fixture results', async () => {
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

  it('AgentsTab add generic creates unapproved file', async () => {
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

describe('AgentsTab approval hardening', () => {
  it('AgentsTab diff view and warnings', async () => {
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
