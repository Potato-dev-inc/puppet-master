// @vitest-environment jsdom
// Settings > Agents > How to run, Chat (terminal): an eligible agent gets a third option in its popup, its note shows
// under the row when the backend sends one, and choosing it saves the lane `terminal_chat`. An agent that can run as
// Chat (terminal) but has no ACP chat still gets a row.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AcpLaneAgent, AcpLaneView, TerminalChatAgent } from '../../../lib/acpChat';
import { AgentLaneRows } from './AgentLaneRows';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const acp = (over: Partial<AcpLaneAgent> = {}): AcpLaneAgent => ({
  id: 'codex',
  label: 'Codex',
  lane: 'terminal',
  effective_lane: 'terminal',
  disabled: false,
  disabled_note: null,
  ...over,
});
const tc = (over: Partial<TerminalChatAgent> = {}): TerminalChatAgent => ({
  id: 'codex',
  label: 'Codex',
  reads_file: true,
  can_resume: false,
  can_answer_prompts: true,
  note: 'It cannot resume old sessions.',
  chosen: false,
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

async function render(view: AcpLaneView, laneSet = vi.fn(async (): Promise<AcpLaneView> => view)) {
  const api = {
    laneGet: async () => view,
    laneSet,
    overlayDetect: vi.fn(),
    overlaySet: vi.fn(),
    overlayRemove: vi.fn(),
  } as unknown as Parameters<typeof AgentLaneRows>[0]['api'];
  await act(async () => root.render(<AgentLaneRows api={api} />));
  await act(async () => {
    await Promise.resolve();
  });
  return { laneSet };
}

const popups = () => [...host.querySelectorAll<HTMLSelectElement>('select')];
const popupFor = (label: string) => popups().find((p) => p.getAttribute('aria-label') === `How ${label} runs`)!;

describe('How to run: Chat (terminal)', () => {
  it('adds the Chat (terminal) option to an eligible agent and shows its note under the row', async () => {
    await render({ labs_enabled: true, agents: [acp()], terminal_chat: [tc()] });
    expect([...popupFor('Codex').options].map((o) => o.textContent)).toEqual(['Terminal', 'Chat (ACP)', 'Chat (terminal)']);
    expect(host.querySelector('[data-testid="agent-terminal-chat-note"]')?.textContent).toContain('It cannot resume old sessions.');
    expect(popupFor('Codex').options[2].textContent).not.toContain('ACP');
  });

  it('gives no third option, and no note, to an agent that is not eligible', async () => {
    await render({ labs_enabled: true, agents: [acp()], terminal_chat: [] });
    expect([...popupFor('Codex').options].map((o) => o.textContent)).toEqual(['Terminal', 'Chat (ACP)']);
    expect(host.querySelector('[data-testid="agent-terminal-chat-note"]')).toBeNull();
  });

  it('saves the terminal_chat lane when the person picks it', async () => {
    const laneSet = vi.fn(async (id: string, lane: string): Promise<AcpLaneView> => ({ labs_enabled: true, agents: [acp({ id, lane: lane as 'terminal', effective_lane: lane as 'terminal' })], terminal_chat: [tc({ chosen: true })] }));
    await render({ labs_enabled: true, agents: [acp()], terminal_chat: [tc()] }, laneSet);
    await act(async () => {
      popupFor('Codex').value = 'terminal_chat';
      popupFor('Codex').dispatchEvent(new Event('change', { bubbles: true }));
    });
    expect(laneSet).toHaveBeenCalledWith('codex', 'terminal_chat');
  });

  it('an agent that can only run as Chat (terminal) still gets a row with Terminal and Chat (terminal)', async () => {
    await render({ labs_enabled: true, agents: [], terminal_chat: [tc({ id: 'gemini', label: 'Gemini', note: '' })] });
    expect([...popupFor('Gemini').options].map((o) => o.textContent)).toEqual(['Terminal', 'Chat (terminal)']);
    expect(popupFor('Gemini').value).toBe('terminal');
  });

  it('shows nothing while Labs is off, even when the backend lists Chat (terminal) agents', async () => {
    await render({ labs_enabled: false, agents: [], terminal_chat: [tc()] });
    expect(host.querySelector('[data-testid="agent-lane"]')).toBeNull();
  });
});
