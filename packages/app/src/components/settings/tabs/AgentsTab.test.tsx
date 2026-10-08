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
      { id: 'kilo', file: KILO, source: 'user', state: 'unapproved', errors: [], fingerprint: 'sha256:abc', shadowed_by: null },
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
    addGeneric: vi.fn(async () => ({
      ...listing,
      definitions: [
        ...listing.definitions,
        { id: 'mycoder', file: 'C:/data/agents/mycoder.toml', source: 'user', state: 'unapproved', errors: [], fingerprint: 'sha256:gen', shadowed_by: null },
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

describe('AgentsTab source toggle / extra dir / consent controls', () => {
  it('lists every source with its enabled state and path', async () => {
    await render(mockApi());
    const rows = host.querySelectorAll('[data-testid="source-row"]');
    expect(Array.from(rows).map((r) => r.getAttribute('data-source'))).toEqual(['bundled', 'user', 'project', 'extra']);
    expect(rows[1].textContent).toContain('C:/data/agents');
    expect(rows[0].textContent).toContain('Enabled');
  });

  it('consent switch invokes the consent command with kind, path and the new value', async () => {
    const api = mockApi();
    await render(api);
    const sw = host.querySelector('[data-source="project"] [role="switch"]');
    expect(sw?.getAttribute('aria-checked')).toBe('false');
    await click(sw);
    expect(api.consent).toHaveBeenCalledWith('project', '', true);
    expect(api.list).toHaveBeenCalledTimes(2);
  });

  it('shows the backend error when consent is refused', async () => {
    const api = mockApi();
    (api.consent as ReturnType<typeof vi.fn>).mockRejectedValueOnce('AGENT_DEF_CONSENT_UNSUPPORTED: not enabled yet');
    await render(api);
    await click(host.querySelector('[data-source="extra"] [role="switch"]'));
    expect(host.textContent).toContain('AGENT_DEF_CONSENT_UNSUPPORTED');
  });

  it('bundled and user sources have no consent switch', async () => {
    await render(mockApi());
    expect(host.querySelector('[data-source="bundled"] [role="switch"]')).toBeNull();
    expect(host.querySelector('[data-source="user"] [role="switch"]')).toBeNull();
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
