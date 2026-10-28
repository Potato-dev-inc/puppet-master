// @vitest-environment jsdom
// "Let {name} see what my agents did today": off unless the view says on, one update call per click, and the failure shown.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakeCompanionTransport, type CompanionView } from '../../../lib/companion';
import { ShareTodayToggle } from './ShareTodayToggle';

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

const toggle = () => host.querySelector<HTMLButtonElement>('button[role="switch"]')!;

const mochi: CompanionView = {
  id: 'mochi',
  name: 'Mochi',
  avatar: 'mochi',
  persona: 'calm',
  agentType: 'claude',
  agentLabel: 'Claude Code',
  folderName: 'mochi',
  state: 'idle',
  stateLine: 'Idle',
  paneId: null,
  needs: null,
  pendingMemory: 0,
  nextCheckin: null,
  restarts: 0,
  autoStart: true,
};

async function renderWith(shareToday: boolean | undefined) {
  const transport = fakeCompanionTransport({ companions: [mochi] });
  const onChanged = vi.fn<(v: CompanionView) => void>();
  await act(async () =>
    root.render(<ShareTodayToggle companionId="mochi" name="Mochi" shareToday={shareToday} transport={transport} onChanged={onChanged} />),
  );
  return { transport, onChanged };
}

describe('ShareTodayToggle', () => {
  it('names the companion in the label', async () => {
    await renderWith(false);
    expect(host.textContent).toContain('Let Mochi see what my agents did today');
    expect(host.textContent).toContain('The list is sent to the AI service of this agent.');
    expect(toggle().getAttribute('aria-label')).toBe('Let Mochi see what my agents did today');
  });

  it('is off by default, also for a view from before the setting', async () => {
    await renderWith(undefined);
    expect(toggle().getAttribute('aria-checked')).toBe('false');
  });

  it('shows on when the companion has it on', async () => {
    await renderWith(true);
    expect(toggle().getAttribute('aria-checked')).toBe('true');
  });

  it('turns it on with one update call and hands back the new view', async () => {
    const { transport, onChanged } = await renderWith(false);
    await act(async () => toggle().click());
    expect(transport.calls.filter((c) => c.method === 'update')).toEqual([{ method: 'update', args: ['mochi', { shareToday: true }] }]);
    expect(onChanged).toHaveBeenCalledTimes(1);
    expect(onChanged.mock.calls[0][0].shareToday).toBe(true);
  });

  it('turns it off again from on', async () => {
    const { transport } = await renderWith(true);
    await act(async () => toggle().click());
    expect(transport.calls.filter((c) => c.method === 'update')).toEqual([{ method: 'update', args: ['mochi', { shareToday: false }] }]);
  });

  it('says why when the save fails and does not report a change', async () => {
    const { transport, onChanged } = await renderWith(false);
    vi.spyOn(transport, 'update').mockRejectedValue(new Error('Desktop shell required: companion_update'));
    await act(async () => toggle().click());
    expect(host.textContent).toContain('Desktop shell required');
    expect(onChanged).not.toHaveBeenCalled();
  });
});
