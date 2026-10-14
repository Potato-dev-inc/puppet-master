// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MemoryApi, MemoryView } from '../../lib/memory-api';
import { ProjectMemory } from './ProjectMemory';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

let host: HTMLDivElement;
let root: Root;

const VIEW: MemoryView = {
  project: 'C:\\work\\shop',
  memory: 'We use Postgres.',
  global_enabled: true,
  project_enabled: true,
  report: null,
  targets: [
    { path: 'CLAUDE.md', agents: ['claude'], status: 'will_add', user_content: true },
    { path: 'AGENTS.md', agents: ['codex', 'opencode'], status: 'will_create', user_content: false },
    { path: '.cursor/rules/puppet-master.mdc', agents: ['cursor_agent'], status: 'in_sync', user_content: false },
  ],
};

function fakeApi(over: Partial<MemoryApi> = {}): MemoryApi {
  return {
    get: vi.fn().mockResolvedValue(VIEW),
    save: vi.fn().mockImplementation(async (text: string) => ({ ...VIEW, memory: text })),
    sync: vi.fn().mockResolvedValue({ ...VIEW, report: { written: ['AGENTS.md'], removed: [], errors: [] } }),
    setProjectEnabled: vi.fn().mockResolvedValue({ ...VIEW, project_enabled: false }),
    globalEnabled: vi.fn().mockResolvedValue(true),
    setGlobalEnabled: vi.fn().mockResolvedValue(true),
    ...over,
  };
}

async function mount(api: MemoryApi) {
  await act(async () => {
    root.render(<ProjectMemory api={api} onClose={() => undefined} />);
  });
  await act(async () => {});
}

const editor = () => host.querySelector('textarea') as HTMLTextAreaElement;
const button = (name: string) => [...host.querySelectorAll('button')].find((b) => b.textContent === name) as HTMLButtonElement;

function type(el: HTMLTextAreaElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!;
  act(() => {
    setter.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe('ProjectMemory', () => {
  it('shows the project memory in a monospace editor with the plain copy', async () => {
    await mount(fakeApi());
    expect(host.textContent).toContain('Every agent in this project reads this.');
    expect(editor().value).toBe('We use Postgres.');
    expect(host.textContent).toContain('shop');
  });

  it('saves only when the text changed, and saves what was typed', async () => {
    const api = fakeApi();
    await mount(api);
    expect(button('Save').disabled).toBe(true);
    type(editor(), 'We use Postgres 16.');
    expect(button('Save').disabled).toBe(false);
    expect(host.textContent).toContain('Unsaved changes');
    await act(async () => button('Save').click());
    expect(api.save).toHaveBeenCalledWith('We use Postgres 16.', undefined);
    expect(button('Save').disabled).toBe(true);
    expect(host.textContent).toContain('Saved.');
  });

  it('lists each agent file with its status and flags files that hold the user own text', async () => {
    await mount(fakeApi());
    const rows = [...host.querySelectorAll('.ck-memory-file')].map((li) => li.textContent ?? '');
    expect(rows).toHaveLength(3);
    expect(rows[0]).toContain('CLAUDE.md');
    expect(rows[0]).toContain('Will add a block');
    expect(rows[0]).toContain('Has your own text');
    expect(rows[1]).toContain('AGENTS.md');
    expect(rows[1]).toContain('Will create');
    expect(rows[1]).not.toContain('Has your own text');
    expect(rows[2]).toContain('In sync');
  });

  it('Sync now calls the backend without discarding unsaved text', async () => {
    const api = fakeApi();
    await mount(api);
    type(editor(), 'draft in progress');
    await act(async () => button('Sync now').click());
    expect(api.sync).toHaveBeenCalledTimes(1);
    expect(editor().value).toBe('draft in progress');
    expect(host.textContent).toContain('Synced.');
  });

  it('shows a sync error from the backend instead of success', async () => {
    const api = fakeApi({
      sync: vi.fn().mockResolvedValue({ ...VIEW, report: { written: [], removed: [], errors: ['CLAUDE.md: the memory markers are damaged'] } }),
    });
    await mount(api);
    await act(async () => button('Sync now').click());
    expect(host.textContent).toContain('markers are damaged');
    expect(host.textContent).not.toContain('Synced.');
  });

  it('turns sharing off for the project and says agent files carry no block', async () => {
    const api = fakeApi();
    await mount(api);
    const box = host.querySelector('input[type="checkbox"]') as HTMLInputElement;
    await act(async () => box.click());
    expect(api.setProjectEnabled).toHaveBeenCalledWith(false, undefined);
    expect(host.textContent).toContain('off for this project');
  });
});
