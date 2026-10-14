// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AGENT_PACK_PREVIEW } from '../../../dev/fixtures';
import { groupAgents, pendingCount, visibleRows, type AgentPackApi } from '../../../lib/agent-pack';
import { AgentPackGroup } from './AgentPackGroup';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

type MockApi = AgentPackApi & { [K in keyof AgentPackApi]: ReturnType<typeof vi.fn> };

function mockApi(overrides: Partial<AgentPackApi> = {}): MockApi {
  return {
    preview: vi.fn(async () => AGENT_PACK_PREVIEW),
    apply: vi.fn(async () => ({
      ...AGENT_PACK_PREVIEW,
      dry_run: false,
      recorded: 9,
      agents: AGENT_PACK_PREVIEW.agents.map((a) => ({
        ...a,
        skills: a.skills && { ...a.skills, written: a.skills.status === 'ready' },
        mcp: a.mcp && { ...a.mcp, written: a.mcp.status === 'ready' },
      })),
    })),
    undo: vi.fn(async () => ({ restored: ['x'], removed: ['a', 'b'], skipped: [], recorded: 0 })),
    ...overrides,
  } as MockApi;
}

let host: HTMLDivElement;
let root: Root;

async function render(api: AgentPackApi) {
  await act(async () => {
    root.render(<AgentPackGroup api={api} />);
  });
}

async function click(el: Element | null | undefined) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
}

const button = (label: string) => Array.from(document.querySelectorAll('button')).find((b) => b.textContent?.trim() === label);

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

describe('AgentPackGroup', () => {
  it('says which agents it found and offers a review, with no Undo before anything is installed', async () => {
    await render(mockApi());
    expect(host.textContent).toContain('Found: Claude Code, Codex, OpenCode.');
    expect(button('Review and install')).toBeTruthy();
    expect(button('Undo')).toBeUndefined();
  });

  it('shows the skills and the connection per agent before anything is written', async () => {
    const api = mockApi();
    await render(api);
    await click(button('Review and install'));
    const rows = Array.from(document.querySelectorAll('[data-testid="pack-agent"]'));
    expect(rows).toHaveLength(3);
    expect(rows[0].textContent).toContain('Claude Code');
    expect(rows[0].textContent).toContain('4 skills to add');
    expect(rows[0].textContent).toContain('Already connected to Puppet Master');
    expect(rows[1].textContent).toContain('Connection to Puppet Master will be added');
    expect(document.querySelector('[data-testid="pack-absent"]')?.textContent).toContain('Cursor Agent');
    expect(api.apply).not.toHaveBeenCalled();
    await click(button('Show details'));
    expect(document.body.textContent).toContain('puppet-master/SKILL.md');
  });

  it('installs, announces where, and then offers Undo', async () => {
    const api = mockApi({ preview: vi.fn().mockResolvedValueOnce(AGENT_PACK_PREVIEW).mockResolvedValueOnce(AGENT_PACK_PREVIEW).mockResolvedValue({ ...AGENT_PACK_PREVIEW, recorded: 9 }) });
    await render(api);
    await click(button('Review and install'));
    await click(button('Install'));
    expect(api.apply).toHaveBeenCalledTimes(1);
    expect(host.textContent).toContain('Added to Claude Code, Codex, OpenCode.');
    expect(host.textContent).toContain('Restart open agents');
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(button('Undo')).toBeTruthy();
  });

  it('undoes and says how many things were put back', async () => {
    const api = mockApi({ preview: vi.fn(async () => ({ ...AGENT_PACK_PREVIEW, recorded: 9 })) });
    await render(api);
    await click(button('Undo'));
    expect(api.undo).toHaveBeenCalledTimes(1);
    expect(host.textContent).toContain('Undone. 3 items were put back as before.');
  });

  it('disables Install when every agent is already set up', async () => {
    const done = {
      ...AGENT_PACK_PREVIEW,
      agents: AGENT_PACK_PREVIEW.agents.map((a) => ({
        ...a,
        skills: a.skills && { ...a.skills, status: a.skills.status === 'ready' ? ('unchanged' as const) : a.skills.status },
        mcp: a.mcp && { ...a.mcp, status: a.mcp.status === 'ready' ? ('unchanged' as const) : a.mcp.status },
      })),
    };
    await render(mockApi({ preview: vi.fn(async () => done) }));
    await click(button('Review and install'));
    expect(button('Install')?.hasAttribute('disabled')).toBe(true);
    expect(document.body.textContent).toContain('Nothing to add.');
  });

  it('shows an error from the backend instead of failing silently', async () => {
    await render(mockApi({ preview: vi.fn(async () => Promise.reject(new Error('no home folder'))) }));
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('no home folder');
  });
});

describe('grouping', () => {
  it('merges agents that share their folders and hides the ones that are not on this computer', () => {
    const shared = {
      ...AGENT_PACK_PREVIEW,
      agents: [
        ...AGENT_PACK_PREVIEW.agents,
        { ...AGENT_PACK_PREVIEW.agents[2], id: 'opencode_native', label: 'OpenCode (API)' },
      ],
    };
    const rows = groupAgents(shared.agents);
    expect(rows.find((r) => r.labels.includes('OpenCode'))?.labels).toEqual(['OpenCode', 'OpenCode (API)']);
    expect(visibleRows(rows).map((r) => r.labels[0])).toEqual(['Claude Code', 'Codex', 'OpenCode']);
    expect(pendingCount(visibleRows(rows))).toBe(3);
  });
});
