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
