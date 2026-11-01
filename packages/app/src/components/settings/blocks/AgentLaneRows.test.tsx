// @vitest-environment jsdom
// Settings > Agents > How to run: a Terminal | Chat (ACP) popup for each agent that has a chat definition, hidden while
// the Labs switch is off, and the reason shown for an agent that cannot run as a chat.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AcpLaneAgent, AcpLaneView } from '../../../lib/acpChat';
import { SECTIONS } from '../catalog';
import { visibleSections } from '../model';
import { AgentLaneRows } from './AgentLaneRows';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const agent = (over: Partial<AcpLaneAgent> = {}): AcpLaneAgent => ({
  id: 'codex',
  label: 'Codex',
  lane: 'terminal',
  effective_lane: 'terminal',
  disabled: false,
  disabled_note: null,
  ...over,
});

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

async function render(api: { laneGet: () => Promise<AcpLaneView>; laneSet: (id: string, lane: string) => Promise<AcpLaneView> }) {
  await act(async () => root.render(<AgentLaneRows api={api} />));
  await act(async () => {
    await Promise.resolve();
  });
}
const popup = () => host.querySelector<HTMLSelectElement>('select');

describe('How to run', () => {
  it('shows nothing while the Labs switch is off', async () => {
    await render({ laneGet: async () => ({ labs_enabled: false, agents: [agent()] }), laneSet: vi.fn() });
    expect(host.querySelector('[data-testid="agent-lane"]')).toBeNull();
  });

  it('shows nothing without a desktop shell', async () => {
    await render({
      laneGet: async () => {
        throw new Error('Desktop shell required: agent_lane_get');
      },
      laneSet: vi.fn(),
    });
    expect(host.textContent).toBe('');
  });

  it('has one popup per agent, Terminal first and by default, and saves the choice', async () => {
    const laneSet = vi.fn(async (id: string, lane: string): Promise<AcpLaneView> => ({
      labs_enabled: true,
      agents: [agent({ id, lane: lane as 'terminal' | 'acp', effective_lane: lane as 'terminal' | 'acp' })],
    }));
    await render({ laneGet: async () => ({ labs_enabled: true, agents: [agent()] }), laneSet });
    expect(host.textContent).toContain('Codex');
    expect(popup()!.value).toBe('terminal');
    expect([...popup()!.options].map((o) => o.textContent)).toEqual(['Terminal', 'Chat (ACP)']);
    await act(async () => {
      popup()!.value = 'acp';
      popup()!.dispatchEvent(new Event('change', { bubbles: true }));
    });
    expect(laneSet).toHaveBeenCalledWith('codex', 'acp');
    await act(async () => {
      await Promise.resolve();
    });
    expect(popup()!.value).toBe('acp');
  });

  it('says why an agent cannot run as a chat, and gives no popup for it', async () => {
    await render({
      laneGet: async () => ({ labs_enabled: true, agents: [agent({ disabled: true, disabled_note: 'Approve this agent first.' })] }),
      laneSet: vi.fn(),
    });
    expect(host.querySelector('[data-testid="agent-lane-note"]')?.textContent).toBe('Approve this agent first.');
    expect(popup()).toBeNull();
  });

  it('shows the sentence when a save is refused, without the code', async () => {
    await render({
      laneGet: async () => ({ labs_enabled: true, agents: [agent()] }),
      laneSet: async () => {
        throw 'ACP_NOT_ENABLED: Chat is not available for this agent.';
      },
    });
    await act(async () => {
      popup()!.value = 'acp';
      popup()!.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await act(async () => {
      await Promise.resolve();
    });
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Chat is not available for this agent.');
  });
});

describe('inside the Settings row wrapper', () => {
  it('the agent rows are direct children, so Settings does not hide the block', async () => {
    const view = { labs_enabled: true, agents: [agent(), agent({ id: 'kilo', label: 'Kilo' })] };
    await render({ laneGet: async () => view, laneSet: vi.fn() });
    expect(host.querySelectorAll(':scope > .st-row').length).toBe(2);
  });
});

describe('Remove chat', () => {
  type Api = Parameters<typeof AgentLaneRows>[0]['api'];
  const removeApi = (agents: AcpLaneAgent[]) => {
    const view = { labs_enabled: true, agents };
    const chatRemove = vi.fn(async (_id: string, _o: { removeAddon: boolean }) => ({ labs_enabled: true, agents: agents.slice(1) }));
    const api = { laneGet: async () => view, laneSet: vi.fn(), chatRemove } as unknown as Api;
    return { api, chatRemove };
  };
  const renderWith = async (api: Api) => {
    await act(async () => root.render(<AgentLaneRows api={api} />));
    await act(async () => {
      await Promise.resolve();
    });
  };
  const dialog = () => document.body.querySelector('[data-testid="remove-chat-dialog"]');
  const dbtn = (text: string) => [...document.body.querySelectorAll<HTMLButtonElement>('[data-testid="remove-chat-dialog"] button')].find((b) => b.textContent === text);

  it('every agent with a chat has Remove chat, and nothing is removed before the sheet is confirmed', async () => {
    const { api, chatRemove } = removeApi([agent(), agent({ id: 'kilo', label: 'Kilo', overlay: true })]);
    await renderWith(api);
    expect([...host.querySelectorAll('button')].filter((b) => b.textContent === 'Remove chat').length).toBe(2);
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Remove chat for Codex"]')!.click());
    expect(dialog()?.textContent).toContain('Remove chat for Codex?');
    expect(dialog()?.textContent).toContain('New sessions will start in a terminal. Your saved chats stay.');
    expect(dialog()?.textContent).not.toContain('add-on');
    await act(async () => dbtn('Cancel')!.click());
    expect(dialog()).toBeNull();
    expect(chatRemove).not.toHaveBeenCalled();
  });

  it('an agent that only has Chat (terminal) gets Remove chat too, and it resets the lane', async () => {
    const tc = { id: 'tchat', label: 'Tchat', reads_file: true, can_resume: false, can_answer_prompts: true, note: 'n', chosen: true };
    const chatRemove = vi.fn(async () => ({ labs_enabled: true, agents: [], terminal_chat: [{ ...tc, chosen: false }] }));
    const api = { laneGet: async () => ({ labs_enabled: true, agents: [], terminal_chat: [tc] }), laneSet: vi.fn(), chatRemove } as unknown as Api;
    await renderWith(api);
    expect(host.querySelector('[aria-label="Remove chat for Tchat"]')).not.toBeNull();
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Remove chat for Tchat"]')!.click());
    await act(async () => dbtn('Remove')!.click());
    await act(async () => {
      await Promise.resolve();
    });
    expect(chatRemove).toHaveBeenCalledWith('tchat', { removeAddon: false });
    expect(host.textContent).toContain('Chat removed for Tchat.');
    expect(host.querySelector('[aria-label="Remove chat for Tchat"]')).toBeNull();
  });

  it("offers to delete the add-on with its size, and sends the person's choice", async () => {
    const { api, chatRemove } = removeApi([agent({ addon: { installed: true, size_mb: 38 } }), agent({ id: 'kilo', label: 'Kilo' })]);
    await renderWith(api);
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Remove chat for Codex"]')!.click());
    expect(dialog()?.textContent).toContain('Also delete the chat add-on (38 MB)');
    const box = document.body.querySelector<HTMLInputElement>('[data-testid="remove-chat-dialog"] input[type="checkbox"]')!;
    expect(box.checked).toBe(false);
    await act(async () => box.click());
    await act(async () => dbtn('Remove')!.click());
    await act(async () => {
      await Promise.resolve();
    });
    expect(chatRemove).toHaveBeenCalledWith('codex', { removeAddon: true });
    expect(dialog()).toBeNull();
    expect(host.textContent).toContain('Chat removed for Codex.');
    expect(host.querySelector('[aria-label="Remove chat for Codex"]')).toBeNull();
  });

  it('removing without the box ticked keeps the add-on', async () => {
    const { api, chatRemove } = removeApi([agent({ addon: { installed: true, size_mb: 2 } })]);
    await renderWith(api);
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Remove chat for Codex"]')!.click());
    await act(async () => dbtn('Remove')!.click());
    expect(chatRemove).toHaveBeenCalledWith('codex', { removeAddon: false });
  });

  it('a failure stays in the sheet in plain words', async () => {
    const { api, chatRemove } = removeApi([agent()]);
    chatRemove.mockRejectedValueOnce(new Error('FEATURE_OFF: chat is switched off in Labs'));
    await renderWith(api);
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Remove chat for Codex"]')!.click());
    await act(async () => dbtn('Remove')!.click());
    await act(async () => {
      await Promise.resolve();
    });
    expect(document.body.querySelector('[data-testid="remove-chat-error"]')?.textContent).toBe('chat is switched off in Labs');
    expect(dbtn('Remove')?.disabled).toBe(false);
  });
});

describe('the settings rows', () => {
  const ids = (settings: { acp_chat_enabled?: boolean } | null) =>
    visibleSections(SECTIONS, settings).flatMap((s) => s.rows.map((r) => r.id));

  it('the How to run row exists only while the Labs switch is on', () => {
    expect(ids({})).not.toContain('agent-lane');
    expect(ids(null)).not.toContain('agent-lane');
    expect(ids({ acp_chat_enabled: false })).not.toContain('agent-lane');
    expect(ids({ acp_chat_enabled: true })).toContain('agent-lane');
  });

  it('the Labs switch row is named in plain words', () => {
    const row = SECTIONS.flatMap((s) => s.rows).find((r) => r.id === 'acp-chat-labs')!;
    expect(row.labs?.name).toBe('Chat sessions (ACP)');
    expect(row.labs?.about).toBe('Run an agent as a chat instead of in a terminal.');
    expect(row.group).toBe('Labs');
  });
});
