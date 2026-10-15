// @vitest-environment jsdom
// Simple mode has a way in: with no agent, the main button opens Set up agents (not Settings), and the
// home screen has a quiet footer with AI helpers, Phone, Settings and a way to switch Simple mode off.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import { MakeSomething } from './MakeSomething';
import { SimpleHome } from './SimpleHome';
import { T } from './copy';

vi.mock('../../lib/agentRegistry', async (orig) => {
  const real = await orig<typeof import('../../lib/agentRegistry')>();
  return { ...real, useAgentRegistry: () => ({ agents: [], dispatchable_ids: [] }) };
});

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

const button = (text: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent === text) as HTMLButtonElement | undefined;

describe('Simple: no agent is ready', () => {
  it('the main button says Set up an AI helper and opens that, not Settings', async () => {
    const open = vi.fn();
    await act(async () => {
      root.render(<MakeSomething onStarted={vi.fn()} onCancel={vi.fn()} onOpenSettings={open} />);
    });
    expect(host.querySelector('[data-testid="no-agent"]')).not.toBeNull();
    expect(button('Open settings')).toBeUndefined();
    const go = button(T.makeAddAgent)!;
    expect(go.textContent).toBe('Set up an AI helper');
    await act(async () => go.click());
    expect(open).toHaveBeenCalledTimes(1);
  });
});

describe('Simple home footer', () => {
  const cockpit = () =>
    ({ registry: { paneList: [], panes: new Map() }, openSettings: vi.fn(async () => undefined), select: vi.fn() }) as unknown as CockpitApi;

  it('offers AI helpers, Phone, Settings and a way to switch Simple mode off', async () => {
    const c = cockpit();
    const onHelpers = vi.fn();
    const onTurnOff = vi.fn();
    await act(async () => {
      root.render(<SimpleHome c={c} onMake={vi.fn()} onOpen={vi.fn()} onHelpers={onHelpers} onTurnOff={onTurnOff} />);
    });
    const foot = host.querySelector('[data-testid="sx-foot"]')!;
    expect(foot.textContent).toBe('AI helpers · Phone · Settings · Simple mode Switch off');
    await act(async () => button(T.footHelpers)!.click());
    expect(onHelpers).toHaveBeenCalledTimes(1);
    await act(async () => button(T.footPhone)!.click());
    expect(c.openSettings).toHaveBeenCalledWith('phone');
    await act(async () => button(T.footSettings)!.click());
    expect(c.openSettings).toHaveBeenLastCalledWith();
    await act(async () => button(T.footTurnOff)!.click());
    expect(onTurnOff).toHaveBeenCalledTimes(1);
  });
});
