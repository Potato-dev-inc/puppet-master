// @vitest-environment jsdom
// The Setup card's Agent row: pick another agent and Move; a move that waits says so and can be cancelled; a moved
// companion says where it came from and can go back.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakeCompanionTransport, type CompanionAgentOption, type CompanionView } from '../../../lib/companion';
import { AgentRow } from './AgentRow';
import { setValue } from './test-helpers';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

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
  vi.restoreAllMocks();
});

const NOW = new Date(2026, 9, 28, 10, 0, 0).getTime();
const FIVE_PM = new Date(2026, 9, 28, 17, 0, 0).getTime();

const mochi: CompanionView = {
  id: 'mochi',
  name: 'Mochi',
  avatar: 'mochi',
  persona: 'calm',
  agentType: 'claude',
  agentLabel: 'Claude Code',
  folderName: 'mochi',
  state: 'idle',
  stateLine: 'Here. Ask me anything.',
  paneId: 'companion-mochi',
  needs: null,
  pendingMemory: 0,
  nextCheckin: null,
  restarts: 0,
  autoStart: true,
};

const agents: CompanionAgentOption[] = [
  { id: 'claude', label: 'Claude Code', ok: true, reason: null },
  { id: 'codex', label: 'Codex', ok: true, reason: null },
];

async function renderRow(view: CompanionView) {
  const transport = fakeCompanionTransport({ companions: [view], agents });
  const onChanged = vi.fn();
  const onNotify = vi.fn();
  await act(async () => {
    root.render(
      <dl>
        <AgentRow companion={view} transport={transport} onChanged={onChanged} onNotify={onNotify} now={() => NOW} />
      </dl>,
    );
  });
  return { transport, onChanged, onNotify };
}

const select = () => host.querySelector<HTMLSelectElement>('select[aria-label="Agent"]')!;
const button = (text: string) => [...host.querySelectorAll('button')].find((b) => b.textContent === text);
const updates = (t: { calls: { method: string; args: unknown[] }[] }) => t.calls.filter((c) => c.method === 'update').map((c) => c.args);

describe('AgentRow', () => {
  it('shows the agent as a picker on the current agent', async () => {
    await renderRow(mochi);
    expect(host.querySelector('dt')?.textContent).toBe('Agent');
    expect(select().value).toBe('claude');
    expect(button('Move')).toBeUndefined();
  });

  it('moves with one update call: the new agent and no end time', async () => {
    const { transport, onChanged } = await renderRow(mochi);
    setValue(select(), 'codex');
    await act(async () => button('Move')!.click());
    expect(updates(transport)).toEqual([['mochi', { agentType: 'codex', untilMs: null }]]);
    expect(onChanged).toHaveBeenCalledTimes(1);
  });

  it('sends the end time when one is given', async () => {
    const { transport } = await renderRow(mochi);
    setValue(select(), 'codex');
    setValue(host.querySelector<HTMLInputElement>('input[type="time"]')!, '17:00');
    await act(async () => button('Move')!.click());
    expect(updates(transport)).toEqual([['mochi', { agentType: 'codex', untilMs: FIVE_PM }]]);
  });

  it('says a move waits, to which agent, and can cancel it by choosing the current agent again', async () => {
    const { transport } = await renderRow({ ...mochi, state: 'working', queuedMove: { agentType: 'codex', agentLabel: 'Codex', untilMs: FIVE_PM } });
    expect(host.textContent).toContain('Moves when Mochi is free');
    expect(host.textContent).toContain('To Codex, until 17:00');
    await act(async () => button('Cancel move')!.click());
    expect(updates(transport)).toEqual([['mochi', { agentType: 'claude' }]]);
  });

  it('says where a moved companion came from and when it goes back, and moves it back on a click', async () => {
    const { transport } = await renderRow({
      ...mochi,
      agentType: 'codex',
      agentLabel: 'Codex',
      previousAgent: 'claude',
      previousAgentLabel: 'Claude Code',
      untilMs: FIVE_PM,
    });
    expect(select().value).toBe('codex');
    expect(host.textContent).toContain('Moved from Claude Code. Goes back at 17:00.');
    await act(async () => button('Move back')!.click());
    expect(updates(transport)).toEqual([['mochi', { agentType: 'claude' }]]);
  });

  it('a lasting move says only where it came from', async () => {
    await renderRow({ ...mochi, agentType: 'codex', agentLabel: 'Codex', previousAgent: 'claude', previousAgentLabel: 'Claude Code' });
    expect(host.textContent).toContain('Moved from Claude Code.');
    expect(host.textContent).not.toContain('Goes back');
  });

  it('shows a refusal as a sentence and reports no change', async () => {
    const { transport, onNotify, onChanged } = await renderRow(mochi);
    vi.spyOn(transport, 'update').mockRejectedValue(new Error('COMPANION_AGENT_UNAVAILABLE: This agent is not installed on this computer.'));
    setValue(select(), 'codex');
    await act(async () => button('Move')!.click());
    expect(onNotify).toHaveBeenCalledWith('COMPANION_AGENT_UNAVAILABLE: This agent is not installed on this computer.');
    expect(onChanged).not.toHaveBeenCalled();
  });

  it('with no agent list it shows the plain label', async () => {
    const transport = fakeCompanionTransport({ companions: [mochi], agents: [] });
    await act(async () => {
      root.render(
        <dl>
          <AgentRow companion={mochi} transport={transport} onChanged={() => {}} onNotify={() => {}} />
        </dl>,
      );
    });
    expect(host.querySelector('select')).toBeNull();
    expect(host.querySelector('dd')?.textContent).toBe('Claude Code');
  });
});
