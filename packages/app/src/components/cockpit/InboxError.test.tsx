// @vitest-environment jsdom
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import { PaneErrorCard } from './InboxError';

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
    root.render(createElement(PaneErrorCard, { c, paneId: 'p1', agentType: 'claude', name: 'Claude Code', project: 'shop', age: 125_000, onOpenDoctor: openDoctor }));
  });
}
const button = (label: string) => [...host.querySelectorAll('button')].find((b) => b.textContent === label);
const buttons = () => [...host.querySelectorAll('button')].map((b) => b.textContent);

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

describe('Inbox error card', () => {
  it('says what happened, what is safe and one next step, in the shared card shape', async () => {
    api.info = { handle: 'h', status: 'failed', receipt: null, error: { code: 'AGENT_EXITED' } };
    await mount();
    expect(host.querySelector('.ib-title')!.textContent).toBe('Claude Code stopped');
    expect(host.querySelector('.ib-glyph [role="img"]')!.getAttribute('aria-label')).toBe('Error');
    expect(host.querySelector('.ib-meta')!.textContent).toBe('shop');
    expect(host.querySelector('.ib-age')!.textContent).toBe('2m');
    expect(host.querySelector('.ib-reason')!.textContent).toBe('It closed unexpectedly. Your files are as it left them.');
    expect(buttons()).toEqual(['Start again', 'Open', 'Ask another agent']);
    expect(host.querySelector('.ck-btn--primary')!.textContent).toBe('Start again');
  });

  it('a declined folder is the person\'s own choice: it says so, offers Start again, and is not drawn as an error', async () => {
    api.info = { handle: 'h', status: 'failed', receipt: null, error: { code: 'WORKSPACE_TRUST_DECLINED' } };
    await mount();
    expect(host.querySelector('.ib-title')!.textContent).toBe("Claude Code didn't start");
    const text = host.querySelector('.ib-reason')!;
    expect(text.textContent).toBe("You chose not to trust this folder, so the task wasn't sent. Start again to be asked once more.");
    // the code goes in a tooltip, never in the text
    expect(text.querySelector('span')!.getAttribute('title')).toBe('WORKSPACE_TRUST_DECLINED');
    expect(host.textContent).not.toContain('WORKSPACE_TRUST_DECLINED');
    // the calm glyph (idle corners), not the X of an error
    expect(host.querySelector('.ib-glyph [role="img"]')!.getAttribute('aria-label')).toBe('Idle');
    await act(async () => button('Start again')!.click());
    expect(newAgent).toHaveBeenCalledWith('claude');
  });

  it('sends a missing agent to Set up agents', async () => {
    api.info = { handle: 'h', status: 'failed', receipt: null, error: { code: 'AGENT_NOT_INSTALLED' } };
    await mount();
    expect(host.querySelector('.ib-reason')!.textContent).toBe("It isn't on this PC yet, so nothing ran.");
    await act(async () => button('Install')!.click());
    expect(openDoctor).toHaveBeenCalled();
  });

  it('a pane that did not get ready opens the pane (one Open button, no duplicate)', async () => {
    api.info = { handle: 'h', status: 'failed', receipt: null, error: { code: 'PANE_NOT_READY' } };
    await mount();
    expect(buttons().filter((t) => t === 'Open')).toHaveLength(1);
    await act(async () => button('Open')!.click());
    expect(select).toHaveBeenCalledWith('p1');
  });

  it('without a known code it says so plainly and keeps Open', async () => {
    api.info = null;
    await mount();
    expect(host.querySelector('.ib-title')!.textContent).toBe('Claude Code stopped');
    expect(host.querySelector('.ib-reason')!.textContent).toBe('Your files are as it left them. Open it to see what happened.');
    await act(async () => button('Open')!.click());
    expect(select).toHaveBeenCalledWith('p1');
  });
});
