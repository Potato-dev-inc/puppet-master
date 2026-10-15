// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { TOOLBELT_IMPORT, TOOLBELT_LISTING, TOOLBELT_PREVIEW } from '../../../dev/fixtures';
import { joinArgs, parsePairs, splitArgs, type ToolbeltApi } from '../../../lib/toolbelt';
import { ToolbeltGroup } from './ToolbeltGroup';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

type MockApi = ToolbeltApi & { [K in keyof ToolbeltApi]: ReturnType<typeof vi.fn> };

function mockApi(overrides: Partial<ToolbeltApi> = {}): MockApi {
  return {
    list: vi.fn(async () => TOOLBELT_LISTING),
    add: vi.fn(async (s) => ({ ...s, id: 'new' })),
    update: vi.fn(async (_id, s) => s),
    remove: vi.fn(async () => undefined),
    syncPreview: vi.fn(async () => TOOLBELT_PREVIEW),
    syncApply: vi.fn(async () => ({
      ...TOOLBELT_PREVIEW,
      dry_run: false,
      hosts: TOOLBELT_PREVIEW.hosts.map((h) => ({ ...h, written: h.status === 'ready' })),
    })),
    importPreview: vi.fn(async () => TOOLBELT_IMPORT),
    importApply: vi.fn(async (refs: string[]) => refs.map((r) => ({ ...TOOLBELT_IMPORT[0].server, id: r }))),
    ...overrides,
  } as MockApi;
}

let host: HTMLDivElement;
let root: Root;

async function render(api: ToolbeltApi) {
  await act(async () => {
    root.render(<ToolbeltGroup api={api} projectPath="C:/work/app" />);
  });
}

async function click(el: Element | null | undefined) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
}

const button = (label: string, within: ParentNode = document) =>
  Array.from(within.querySelectorAll('button')).find((b) => b.textContent?.trim() === label);

async function type(el: Element | null, value: string) {
  expect(el).toBeTruthy();
  const input = el as HTMLInputElement | HTMLTextAreaElement;
  const proto = input instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  await act(async () => {
    Object.getOwnPropertyDescriptor(proto, 'value')!.set!.call(input, value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

const labelled = (text: string) =>
  Array.from(document.querySelectorAll('label'))
    .find((l) => l.textContent?.trim().startsWith(text))
    ?.querySelector('input,textarea,select') ?? null;

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

describe('ToolbeltGroup', () => {
  it('lists the servers with their scope and what they run, and says where secrets go', async () => {
    await render(mockApi());
    const rows = host.querySelectorAll('[aria-label="Servers"] li');
    expect(rows).toHaveLength(4);
    expect(rows[0].textContent).toContain('Browser');
    expect(rows[0].textContent).toContain('npx -y @playwright/mcp');
    expect(rows[2].textContent).toContain('https://mcp.context7.com/mcp');
    expect(rows[3].textContent).toContain('This project');
    expect(host.textContent).toContain('plain text');
    expect(host.textContent).toContain('Not supported: Kilo');
    const switches = host.querySelectorAll('[role="switch"]');
    expect(Array.from(switches).map((s) => s.getAttribute('aria-checked'))).toEqual(['true', 'true', 'true', 'false']);
  });

  it('shows a plain empty state', async () => {
    await render(mockApi({ list: vi.fn(async () => ({ ...TOOLBELT_LISTING, servers: [] })) }));
    expect(host.textContent).toContain('No servers yet');
  });

  it('flips the enable switch through update, keeping the rest of the server', async () => {
    const api = mockApi();
    await render(api);
    await click(host.querySelector('[role="switch"][aria-label="Use Linear"]'));
    expect(api.update).toHaveBeenCalledWith('linear', expect.objectContaining({ id: 'linear', enabled: true, url: 'https://mcp.linear.app/mcp' }));
  });

  it('adds a command server from the form', async () => {
    const api = mockApi();
    await render(api);
    await click(button('Add server'));
    await type(labelled('Name'), 'Files');
    await type(labelled('Command'), 'npx');
    await type(labelled('Arguments'), '-y "my pkg" --flag');
    await type(labelled('Environment variables'), 'TOKEN=abc\nMODE=dev');
    await click(button('Save'));
    expect(api.add).toHaveBeenCalledWith({
      id: '',
      name: 'Files',
      transport: 'stdio',
      command: 'npx',
      args: ['-y', 'my pkg', '--flag'],
      env: { TOKEN: 'abc', MODE: 'dev' },
      enabled: true,
      scope: 'global',
    });
    expect(host.querySelector('form')).toBeNull();
  });

  it('adds a web address server and shows the backend message when it is refused', async () => {
    const api = mockApi({
      add: vi.fn(async () => Promise.reject(new Error('Enter the server address, starting with http:// or https://.'))) as never,
    });
    await render(api);
    await click(button('Add server'));
    await click(button('Web address'));
    await type(labelled('Name'), 'Docs');
    await type(labelled('Address'), 'nope');
    await type(labelled('Headers'), 'Authorization: Bearer x');
    await click(button('Save'));
    expect(api.add).toHaveBeenCalledWith(expect.objectContaining({ transport: 'http', url: 'nope', headers: { Authorization: 'Bearer x' } }));
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('starting with http://');
    expect(host.querySelector('form')).not.toBeNull();
  });

  it('removes a server', async () => {
    const api = mockApi();
    await render(api);
    await click(host.querySelector('[aria-label="Remove Postgres"]'));
    expect(api.remove).toHaveBeenCalledWith('postgres');
  });
});

describe('Apply preview sheet', () => {
  it('shows per-agent changes, skipped agents and unsupported agents', async () => {
    const api = mockApi();
    await render(api);
    await click(button('Apply to all agents'));
    const dialog = document.querySelector('[role="dialog"][aria-label="Apply to all agents"]')!;
    expect(dialog).toBeTruthy();
    expect(api.syncPreview).toHaveBeenCalledWith({ project: 'C:/work/app', includePm: false });
    const hosts = dialog.querySelectorAll('[data-testid="toolbelt-host"]');
    expect(hosts).toHaveLength(4);
    expect(hosts[0].textContent).toContain('Claude Code');
    expect(hosts[0].textContent).toContain('2 to add');
    expect(hosts[0].textContent).toContain('Postgres');
    expect(hosts[1].textContent).toContain('1 to remove');
    expect(hosts[1].textContent).toContain('Old search');
    expect(hosts[2].textContent).toContain('OpenCode, OpenCode (API)');
    expect(hosts[2].textContent).toContain('Already up to date');
    expect(hosts[3].textContent).toContain('Not installed on this computer');
    expect(dialog.querySelector('[data-testid="toolbelt-unsupported"]')?.textContent).toContain('Kilo');
    expect(dialog.textContent).toContain('2 files will change.');
    // The diff is behind a link and never shows a secret.
    expect(dialog.querySelector('pre')).toBeNull();
    await click(button('Show file changes', dialog));
    expect(dialog.querySelector('pre')?.textContent).toContain('"DB_PASSWORD": "********"');
  });

  it('applies, closes and says how many files changed', async () => {
    const api = mockApi();
    await render(api);
    await click(button('Apply to all agents'));
    await click(button('Apply'));
    expect(api.syncApply).toHaveBeenCalledWith({ project: 'C:/work/app', includePm: false });
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(host.textContent).toContain('Updated 2 config files. Restart open agents');
  });

  it('cannot apply when nothing would change, and cancel writes nothing', async () => {
    const quiet = { ...TOOLBELT_PREVIEW, hosts: TOOLBELT_PREVIEW.hosts.map((h) => ({ ...h, status: 'unchanged' as const })) };
    const api = mockApi({ syncPreview: vi.fn(async () => quiet) });
    await render(api);
    await click(button('Apply to all agents'));
    const apply = button('Apply', document.querySelector('[role="dialog"]')!) as HTMLButtonElement;
    expect(apply.disabled).toBe(true);
    expect(document.body.textContent).toContain('Nothing to change.');
    await click(button('Cancel'));
    expect(api.syncApply).not.toHaveBeenCalled();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  });

  it('re-previews when Puppet Master itself is included', async () => {
    const api = mockApi();
    await render(api);
    await click(button('Apply to all agents'));
    await click(labelled('Also register Puppet Master'));
    expect(api.syncPreview).toHaveBeenLastCalledWith({ project: 'C:/work/app', includePm: true });
  });
});

describe('Import sheet', () => {
  it('offers what agents already have and adds the picked ones', async () => {
    const api = mockApi();
    await render(api);
    await click(button('Import from agents'));
    const dialog = document.querySelector('[role="dialog"][aria-label="Import from agents"]')!;
    const boxes = dialog.querySelectorAll<HTMLInputElement>('input[type="checkbox"]');
    expect(boxes).toHaveLength(2);
    expect(boxes[0].checked).toBe(true);
    expect(boxes[1].disabled).toBe(true);
    expect(dialog.textContent).toContain('Already in your toolbelt');
    expect(dialog.textContent).toContain('Found in Claude Code, Codex');
    await click(button('Add to toolbelt', dialog));
    expect(api.importApply).toHaveBeenCalledWith(['global|claude|github'], 'C:/work/app');
    expect(host.textContent).toContain('Added 1 server to your toolbelt.');
  });
});

describe('helpers', () => {
  it('splits and joins arguments, parses pairs', () => {
    expect(splitArgs(`-y "a b" 'c d' e`)).toEqual(['-y', 'a b', 'c d', 'e']);
    expect(joinArgs(['-y', 'a b'])).toBe('-y "a b"');
    expect(parsePairs('A=1\nbad\n=x\nB = two=2')).toEqual({ A: '1', B: 'two=2' });
    expect(parsePairs('Authorization: Bearer x', ':')).toEqual({ Authorization: 'Bearer x' });
  });
});
