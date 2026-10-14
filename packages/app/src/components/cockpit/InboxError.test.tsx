// @vitest-environment jsdom
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import { PaneErrorBody } from './InboxError';

const api = vi.hoisted(() => ({ info: null as null | object }));
vi.mock('../../lib/cockpit-api', () => ({
  cockpitApi: { paneRunInfo: () => Promise.resolve(api.info) },
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
const newAgent = vi.fn();
const select = vi.fn();
const openDoctor = vi.fn();
const c = { newAgent, select } as unknown as CockpitApi;

async function mount() {
  await act(async () => {
    root.render(createElement(PaneErrorBody, { c, paneId: 'p1', agentType: 'claude', name: 'Claude Code', onOpenDoctor: openDoctor }));
  });
}
const button = (label: string) => [...host.querySelectorAll('button')].find((b) => b.textContent === label);

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  newAgent.mockClear();
  select.mockClear();
  openDoctor.mockClear();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe('Inbox error card body', () => {
  it('says why a declined folder stopped the run and offers Start again', async () => {
    api.info = { handle: 'h', status: 'failed', receipt: null, error: { code: 'WORKSPACE_TRUST_DECLINED' } };
    await mount();
    const text = host.querySelector('.ck-inbox-q')!;
    expect(text.textContent).toBe("Claude Code closed because you chose Don't trust. The task wasn't sent.");
    expect(text.getAttribute('title')).toBe('WORKSPACE_TRUST_DECLINED');
    expect(host.textContent).not.toContain('Open the terminal');
    await act(async () => button('Start again')!.click());
    expect(newAgent).toHaveBeenCalledWith('claude');
  });
  it('sends a missing agent to Set up agents', async () => {
    api.info = { handle: 'h', status: 'failed', receipt: null, error: { code: 'AGENT_NOT_INSTALLED' } };
    await mount();
    expect(host.querySelector('.ck-inbox-q')!.textContent).toBe("Claude Code isn't installed on this PC.");
    await act(async () => button('Install')!.click());
    expect(openDoctor).toHaveBeenCalled();
  });
  it('a pane that did not get ready opens the pane (one Open button, no duplicate)', async () => {
    api.info = { handle: 'h', status: 'failed', receipt: null, error: { code: 'PANE_NOT_READY' } };
    await mount();
    await act(async () => button('Open pane')!.click());
    expect(select).toHaveBeenCalledWith('p1');
    expect(button('Open')).toBeUndefined();
  });
  it('without a known code it says so plainly and keeps Open', async () => {
    api.info = null;
    await mount();
    expect(host.querySelector('.ck-inbox-q')!.textContent).toBe('Something went wrong and Claude Code stopped. Open it to see what happened.');
    await act(async () => button('Open')!.click());
    expect(select).toHaveBeenCalledWith('p1');
  });
});
