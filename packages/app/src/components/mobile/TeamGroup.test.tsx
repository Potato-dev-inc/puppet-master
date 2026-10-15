// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { BridgeClient, TeamCardsReply } from '../../lib/bridge-types';
import { phoneTeamReply, PHONE_TEAM_CARDS, PHONE_TEAM_JOB } from '../../dev/phone-team-fixtures';
import { TeamGroup } from './TeamGroup';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const MAYA = PHONE_TEAM_CARDS[0];
const SAM = PHONE_TEAM_CARDS[1];

function fakeBridge(reply: TeamCardsReply) {
  return {
    teamCards: vi.fn(async () => reply),
    teamDismiss: vi.fn(async (_id: string) => ({ ok: true })),
    teamPull: vi.fn(async (_id: string) => ({ ok: true, result: 'Pulled.' })),
    teamQueue: vi.fn(async (_id: string, _pane: string, _when: string) => ({ ok: true, result: PHONE_TEAM_JOB })),
    teamApply: vi.fn(async (_id: string) => ({ ok: true, result: 'Applied.' })),
    teamDiscard: vi.fn(async (_id: string) => ({ ok: true })),
  };
}

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
  vi.useRealTimers();
});

async function mount(bridge: ReturnType<typeof fakeBridge> | object, bridgeReady = true) {
  await act(async () => {
    root.render(<TeamGroup bridge={bridge as unknown as BridgeClient} bridgeReady={bridgeReady} />);
  });
  await act(async () => {});
}

const buttons = (scope: ParentNode = host) => Array.from(scope.querySelectorAll('button')).map((b) => b.textContent);
const card = (id: string) => host.querySelector<HTMLElement>(`[data-card-id="${id}"]`)!;
const byText = (text: string, scope: ParentNode = host) =>
  Array.from(scope.querySelectorAll('button')).find((b) => b.textContent === text) as HTMLButtonElement;
async function click(el: Element) {
  await act(async () => {
    (el as HTMLElement).click();
  });
}

const WATCH_LINE = 'This phone can see team updates but not act on them. Change it on your PC: Settings › Phone.';

describe('phone TeamGroup', () => {
  it('renders nothing when Team is off', async () => {
    await mount(fakeBridge({ ...phoneTeamReply(), enabled: false }));
    expect(host.innerHTML).toBe('');
  });

  it('renders nothing on a bridge without the route, or with nothing to show', async () => {
    await mount({});
    expect(host.innerHTML).toBe('');
    await mount(fakeBridge({ ...phoneTeamReply(), cards: [], jobs: [] }));
    expect(host.innerHTML).toBe('');
  });

  it('does not ask the bridge before it is ready', async () => {
    const bridge = fakeBridge(phoneTeamReply());
    await mount(bridge, false);
    expect(bridge.teamCards).not.toHaveBeenCalled();
    expect(host.innerHTML).toBe('');
  });

  it('shows a card with its title, meta, quoted note and time', async () => {
    await mount(fakeBridge(phoneTeamReply()));
    expect(host.querySelector('h2.ph-sec')?.textContent).toBe('Team');
    const maya = card(MAYA.id);
    expect(maya.querySelector('.ph-team-title')?.textContent).toBe("Maya's Claude Code pushed feat/auth");
    expect(maya.textContent).toContain('billing-service');
    expect(maya.querySelector('.ph-team-note')?.textContent).toBe('“Touches the session model, careful with your billing branch”');
    expect(maya.querySelector('.ph-team-when')?.textContent).toBe('4m ago');
  });

  it('watch-only phone: no buttons, one explanatory line', async () => {
    await mount(fakeBridge(phoneTeamReply({ can: { dismiss: false, act: false } })));
    expect(host.querySelectorAll('button')).toHaveLength(0);
    expect(host.textContent?.split(WATCH_LINE)).toHaveLength(2);
  });

  it('answer-only phone: just Dismiss on a card, no explanatory line', async () => {
    await mount(fakeBridge(phoneTeamReply({ can: { dismiss: true, act: false } })));
    expect(buttons(card(MAYA.id))).toEqual(['Dismiss']);
    expect(buttons(card(SAM.id))).toEqual(['Dismiss']);
    expect(host.textContent).not.toContain(WATCH_LINE);
    expect(buttons(host.querySelector('[data-job-id]')!)).toEqual([]);
  });

  it('control phone: Queue my agent or Pull now, plus Dismiss, never more than two buttons', async () => {
    await mount(fakeBridge(phoneTeamReply()));
    expect(buttons(card(MAYA.id))).toEqual(['Queue my agent', 'Dismiss']);
    expect(buttons(card(SAM.id))).toEqual(['Pull now', 'Dismiss']);
    expect(buttons(host.querySelector('[data-job-id]')!)).toEqual(['Apply to my branch', 'Discard']);
  });

  it('offers no Queue button when no agent is open in the project', async () => {
    const reply = phoneTeamReply();
    reply.cards = [{ ...MAYA, panes: [] }];
    reply.jobs = [];
    await mount(fakeBridge(reply));
    expect(buttons(card(MAYA.id))).toEqual(['Dismiss']);
    expect(card(MAYA.id).textContent).toContain('No agent is open in this project.');
  });

  it('Dismiss calls the dismiss route for that card', async () => {
    const bridge = fakeBridge(phoneTeamReply());
    await mount(bridge);
    await click(byText('Dismiss', card(MAYA.id)));
    expect(bridge.teamDismiss).toHaveBeenCalledWith(MAYA.id);
    expect(bridge.teamQueue).not.toHaveBeenCalled();
  });

  it('Pull now calls the pull route and shows its result', async () => {
    const bridge = fakeBridge(phoneTeamReply());
    await mount(bridge);
    await click(byText('Pull now', card(SAM.id)));
    expect(bridge.teamPull).toHaveBeenCalledWith(SAM.id);
    expect(host.querySelector('[role="status"]')?.textContent).toBe('Pulled.');
  });

  it('Queue my agent asks first and queues for the first listed pane when idle', async () => {
    const bridge = fakeBridge(phoneTeamReply());
    await mount(bridge);
    await click(byText('Queue my agent'));
    expect(bridge.teamQueue).not.toHaveBeenCalled();
    const sheet = host.querySelector('[role="alertdialog"]')!;
    expect(sheet.textContent).toContain(
      'Queue Claude Code to bring feat/auth into feat/billing? It works in a separate copy and asks you before anything lands.',
    );
    await click(byText('Queue it', sheet));
    expect(bridge.teamQueue).toHaveBeenCalledWith(MAYA.id, MAYA.panes[0].paneId, 'idle');
    expect(host.querySelector('[role="alertdialog"]')).toBeNull();
  });

  it('Cancel on the queue sheet calls nothing', async () => {
    const bridge = fakeBridge(phoneTeamReply());
    await mount(bridge);
    await click(byText('Queue my agent'));
    await click(byText('Cancel'));
    expect(bridge.teamQueue).not.toHaveBeenCalled();
    expect(host.querySelector('[role="alertdialog"]')).toBeNull();
  });

  it('Apply to my branch asks first, then calls the apply route', async () => {
    const bridge = fakeBridge(phoneTeamReply());
    await mount(bridge);
    await click(byText('Apply to my branch'));
    expect(bridge.teamApply).not.toHaveBeenCalled();
    const sheet = host.querySelector('[role="alertdialog"]')!;
    expect(sheet.textContent).toContain('Fast-forward feat/billing? Nothing is pushed.');
    await click(byText('Apply', sheet));
    expect(bridge.teamApply).toHaveBeenCalledWith(PHONE_TEAM_JOB.id);
  });

  it('Discard calls the discard route', async () => {
    const bridge = fakeBridge(phoneTeamReply());
    await mount(bridge);
    await click(byText('Discard'));
    expect(bridge.teamDiscard).toHaveBeenCalledWith(PHONE_TEAM_JOB.id);
  });

  it('shows text only for jobs that are not ready', async () => {
    const reply = phoneTeamReply();
    reply.cards = [];
    reply.jobs = [{ ...PHONE_TEAM_JOB, state: 'working', agentRan: true, conflicts: ['a.ts'] }];
    await mount(fakeBridge(reply));
    expect(host.querySelectorAll('button')).toHaveLength(0);
    expect(host.textContent).toContain('Claude Code is merging fix/tax-rounding');
  });

  it('says some updates may be missing', async () => {
    await mount(fakeBridge(phoneTeamReply({ missing: 2 })));
    expect(host.textContent).toContain('Some team updates may be missing.');
  });

  it('shows a plain sentence when the PC refuses', async () => {
    const bridge = fakeBridge(phoneTeamReply());
    bridge.teamDismiss.mockRejectedValueOnce(new Error('bridge POST /team/cards/x/dismiss -> 409: {"error":"That update is already gone."}'));
    await mount(bridge);
    await click(byText('Dismiss', card(MAYA.id)));
    expect(host.textContent).toContain('That update is already gone.');
    bridge.teamDismiss.mockRejectedValueOnce(new Error('bridge POST /x -> 403: {"error":"not_allowed"}'));
    await click(byText('Dismiss', card(MAYA.id)));
    expect(host.textContent).toContain("This phone isn't allowed to do that.");
    expect(host.textContent).not.toContain('not_allowed');
  });

  it('renders a teammate note with HTML as plain text', async () => {
    const reply = phoneTeamReply();
    const evil = '<img src=x onerror=alert(1)> **bold** <b>hi</b>';
    reply.cards = [{ ...MAYA, event: { ...MAYA.event, note: evil } }];
    reply.jobs = [];
    await mount(fakeBridge(reply));
    const note = host.querySelector('.ph-team-note')!;
    expect(note.textContent).toBe(`“${evil}”`);
    expect(note.children).toHaveLength(0);
    expect(host.querySelector('img')).toBeNull();
  });

  it('polls every 15 seconds and again when the page becomes visible', async () => {
    vi.useFakeTimers();
    const bridge = fakeBridge(phoneTeamReply());
    await mount(bridge);
    expect(bridge.teamCards).toHaveBeenCalledTimes(1);
    await act(async () => {
      vi.advanceTimersByTime(15_000);
    });
    expect(bridge.teamCards).toHaveBeenCalledTimes(2);
    await act(async () => {
      document.dispatchEvent(new Event('visibilitychange'));
    });
    expect(bridge.teamCards).toHaveBeenCalledTimes(3);
  });

  it('shows no raw ids', async () => {
    await mount(fakeBridge(phoneTeamReply()));
    expect(host.textContent).not.toContain(MAYA.id);
    expect(host.textContent).not.toContain(PHONE_TEAM_JOB.id);
    expect(host.textContent).not.toContain(MAYA.panes[0].paneId);
  });
});
