// @vitest-environment jsdom
// Settings > Labs > Team: create / join / waiting / admin views, the confirm sheets and the invite sheet.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { teamConfig } from '../../../dev/team-fixtures';
import type { TeamApi, TeamConfigView } from '../../../lib/team';
import { buttonByText, click, hasText, settle, type as typeInto } from '../test-utils';
import { TeamSettings } from './TeamSettings';

vi.mock('qrcode', () => ({ default: { toCanvas: vi.fn(async () => undefined) } }));

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

const NOT_ON_TEAM: Partial<TeamConfigView> = { enabled: false, team: '', teamName: '', relay: '', name: '', members: [], joins: [], admin: false, transport: null, link: null };

function fakeApi(view: TeamConfigView) {
  const calls: string[] = [];
  const rec =
    <T,>(name: string, value: T) =>
    async (...args: unknown[]) => {
      calls.push(`${name}(${args.map((a) => JSON.stringify(a)).join(',')})`);
      return value;
    };
  const api = {
    calls,
    configGet: rec('configGet', view),
    configSet: rec('configSet', view),
    create: rec('create', teamConfig()),
    invite: rec('invite', { link: 'pm://team/join?t=Acme&k=abc123', code: 'KQ7M-4XPD', expiresAtMs: Date.now() + 600_000 }),
    join: rec('join', teamConfig({ team: '', joining: 'Acme' })),
    leave: rec('leave', teamConfig(NOT_ON_TEAM)),
    approveJoin: rec('approveJoin', teamConfig({ joins: [] })),
    declineJoin: rec('declineJoin', teamConfig({ joins: [] })),
    removeMember: rec('removeMember', teamConfig()),
    confirmGithub: rec('confirmGithub', teamConfig()),
  };
  return api as typeof api & TeamApi;
}

async function mount(view: TeamConfigView) {
  const api = fakeApi(view);
  const onSaved = vi.fn();
  await act(async () => root.render(<TeamSettings api={api} onSaved={onSaved} />));
  await settle(5);
  return { api, onSaved };
}

/** The input in the row whose label text is `label`. */
const field = (label: string) =>
  [...host.querySelectorAll('label')].find((l) => l.querySelector('span')?.textContent === label)?.querySelector('input, textarea') as HTMLInputElement;
const sheet = () => host.querySelector('.st-sheet');
const sheetButton = (text: string) => [...(sheet()?.querySelectorAll('button') ?? [])].find((b) => b.textContent?.trim() === text) as HTMLButtonElement;

describe('not on a team', () => {
  it('create: sends the team name, my name and the relay, and nothing else', async () => {
    const { api } = await mount(teamConfig(NOT_ON_TEAM));
    expect(hasText(host, 'Create a team')).toBe(true);
    expect(hasText(host, 'Join with an invite')).toBe(true);
    expect(buttonByText(host, 'Create team')!.disabled).toBe(true);
    await typeInto(field('Team name'), 'Acme');
    await typeInto(field('Relay address'), 'https://relay.example.com');
    // The "Your name" field exists in both forms; the first is the create form.
    await typeInto(host.querySelectorAll<HTMLInputElement>('label input[placeholder="Maya"]')[0], 'Maya');
    await click(buttonByText(host, 'Create team'));
    await settle();
    expect(api.calls.filter((c) => c.startsWith('create'))).toEqual(['create({"teamName":"Acme","name":"Maya","relay":"https://relay.example.com","folder":""})']);
  });

  it('create: a shared folder replaces the relay address', async () => {
    const { api } = await mount(teamConfig(NOT_ON_TEAM));
    await click(host.querySelector('[role="switch"][aria-label="Use a shared folder instead"]'));
    expect(field('Relay address')).toBeUndefined();
    await typeInto(field('Team name'), 'Acme');
    await typeInto(field('Shared folder'), 'D:\\team\\wire');
    await typeInto(host.querySelectorAll<HTMLInputElement>('label input[placeholder="Maya"]')[0], 'Maya');
    await click(buttonByText(host, 'Create team'));
    await settle();
    expect(api.calls.find((c) => c.startsWith('create'))).toBe('create({"teamName":"Acme","name":"Maya","relay":"","folder":"D:\\\\team\\\\wire"})');
  });

  it('join: the code is shown as ABCD-EFGH and the join is sent with it', async () => {
    const { api } = await mount(teamConfig(NOT_ON_TEAM));
    expect(buttonByText(host, 'Ask to join')!.disabled).toBe(true);
    await typeInto(field('Invite link'), 'pm://team/join?t=Acme&k=abc');
    await typeInto(field('Code'), 'abcdefgh');
    expect(field('Code').value).toBe('ABCD-EFGH');
    await typeInto(host.querySelector<HTMLInputElement>('label input[placeholder="Leo"]')!, 'Leo');
    await click(buttonByText(host, 'Ask to join'));
    await settle();
    expect(api.calls.find((c) => c.startsWith('join'))).toBe('join({"link":"pm://team/join?t=Acme&k=abc","code":"ABCD-EFGH","name":"Leo","github":null})');
    // The answer says we are waiting for the admin.
    expect(hasText(host, "Waiting for Acme's admin to let you in.")).toBe(true);
  });

  it('a settings file that could not be read is said plainly', async () => {
    await mount(teamConfig({ ...NOT_ON_TEAM, readError: 'expected value at line 1' }));
    const alert = host.querySelector('[role="alert"]')!;
    expect(alert.textContent).toContain("couldn't be read");
    expect(alert.textContent).toContain('team.json.unreadable');
    expect(alert.textContent).toContain('expected value at line 1');
  });
});

describe('waiting for the admin', () => {
  it('says who we wait for, and Cancel leaves', async () => {
    const { api } = await mount(teamConfig({ ...NOT_ON_TEAM, joining: 'Acme' }));
    expect(hasText(host, "Waiting for Acme's admin to let you in.")).toBe(true);
    expect(hasText(host, 'Create a team')).toBe(false);
    await click(buttonByText(host, 'Cancel'));
    await settle();
    expect(api.calls).toContain('leave()');
    expect(hasText(host, 'Create a team')).toBe(true);
  });
});

describe('on a team as the admin', () => {
  it('shows the team, the connection, the keys line and the people in words', async () => {
    await mount(teamConfig());
    expect(hasText(host, "Acme · You're the admin")).toBe(true);
    expect(hasText(host, 'Relay · live')).toBe(true);
    expect(hasText(host, 'Keys are kept in Windows Credential Manager.')).toBe(true);
    expect(hasText(host, 'the relay sees only who sends when, never names, branches or code.')).toBe(true);
    expect(hasText(host, 'Leo (you)')).toBe(true);
    expect(hasText(host, 'Admin')).toBe(true);
    expect(hasText(host, 'Can answer')).toBe(true);
    expect(hasText(host, 'Watching')).toBe(true);
    expect(hasText(host, 'GitHub: maya-dev')).toBe(true);
    expect(hasText(host, 'GitHub: maya-dev (claimed)')).toBe(false);
    expect(hasText(host, 'GitHub: samc (claimed)')).toBe(true);
    // Never an id.
    for (const id of ['team-7f3a91c2', 'm-maya', 'm-sam', 'm-leo', 'j-priya']) expect(host.textContent).not.toContain(id);
  });

  it('a claimed GitHub login can be confirmed by the admin; a confirmed one cannot', async () => {
    const { api } = await mount(teamConfig());
    const confirms = [...host.querySelectorAll('button')].filter((b) => b.textContent === 'Confirm GitHub');
    expect(confirms.length).toBe(1);
    await click(confirms[0] as HTMLButtonElement);
    await settle();
    expect(api.calls).toContain('confirmGithub("m-sam")');
  });

  it('flags join requests when more than one person used the same code', async () => {
    const base = teamConfig();
    const warning = "Someone else also used this invite's code. Only let in the person you invited.";
    await mount({ ...base, joins: base.joins.map((j) => ({ ...j, warning })) });
    expect(host.querySelector('.st-team-warning[role="alert"]')?.textContent).toBe(warning);
  });

  it('says how the relay is doing, or that it is a shared folder', async () => {
    await mount(teamConfig({ link: 'offline' }));
    expect(hasText(host, "Relay · can't reach it")).toBe(true);
    await act(async () => root.unmount());
    root = createRoot(host);
    await mount(teamConfig({ transport: 'folder', link: null }));
    expect(hasText(host, 'Shared folder')).toBe(true);
  });

  it('a pending request: Let in approves that one, No declines', async () => {
    const { api } = await mount(teamConfig());
    expect(hasText(host, 'Priya wants to join')).toBe(true);
    expect(hasText(host, 'GitHub: priya-dev (claimed)')).toBe(true);
    expect(hasText(host, "Priya's MacBook")).toBe(true);
    await click(buttonByText(host, 'Let Priya in'));
    await settle();
    expect(api.calls).toContain('approveJoin("j-priya")');
    expect(hasText(host, 'Priya wants to join')).toBe(false);
  });

  it('No on a request declines it', async () => {
    const { api } = await mount(teamConfig());
    await click(buttonByText(host, 'No'));
    await settle();
    expect(api.calls).toContain('declineJoin("j-priya")');
  });

  it('Remove asks first, says what it does, and removes only after the confirm', async () => {
    const { api } = await mount(teamConfig());
    const removes = [...host.querySelectorAll('button')].filter((b) => b.textContent === 'Remove');
    expect(removes).toHaveLength(2); // Maya and Sam, never me
    await click(removes[0]);
    expect(sheet()!.textContent).toContain('Remove Maya?');
    expect(sheet()!.textContent).toContain("They keep what they already received. The team key changes, so they can't read anything new.");
    expect(api.calls.some((c) => c.startsWith('removeMember'))).toBe(false);
    await click(sheetButton('Remove'));
    await settle();
    expect(api.calls).toContain('removeMember("m-maya")');
    expect(sheet()).toBeNull();
  });

  it('Cancel on the Remove sheet removes nobody', async () => {
    const { api } = await mount(teamConfig());
    await click([...host.querySelectorAll('button')].find((b) => b.textContent === 'Remove'));
    await click(sheetButton('Cancel'));
    expect(api.calls.some((c) => c.startsWith('removeMember'))).toBe(false);
  });

  it('Invite someone shows the code, the link and how to send them, and never the team id', async () => {
    const { api } = await mount(teamConfig());
    await click(buttonByText(host, 'Invite someone'));
    await settle(5);
    expect(api.calls).toContain('invite()');
    expect(host.querySelector('.st-invite-code')!.textContent).toBe('KQ7M-4XPD');
    expect((host.querySelector('input[aria-label="Invite link"]') as HTMLInputElement).value).toBe('pm://team/join?t=Acme&k=abc123');
    expect(sheet()!.textContent).toContain('Send the link and the code separately. The code works once, for 10 minutes. You\'ll be asked before anyone gets in.');
    expect(sheet()!.textContent).toMatch(/Expires at/);
    expect(host.querySelector('canvas')).not.toBeNull();
    expect(host.textContent).not.toContain('team-7f3a91c2');
    await click(sheetButton('Done'));
    expect(sheet()).toBeNull();
  });

  it('Copy puts the link on the clipboard', async () => {
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    await mount(teamConfig());
    await click(buttonByText(host, 'Invite someone'));
    await settle(5);
    await click(sheetButton('Copy'));
    expect(writeText).toHaveBeenCalledWith('pm://team/join?t=Acme&k=abc123');
  });

  it('the Use Team switch saves with the projects as they are typed', async () => {
    const { api, onSaved } = await mount(teamConfig({ enabled: false }));
    await typeInto(field('Projects') as unknown as HTMLTextAreaElement, 'C:\\work\\billing\n\nC:\\work\\web');
    await click(host.querySelector('[role="switch"][aria-label="Use Team"]'));
    await settle();
    expect(api.calls.find((c) => c.startsWith('configSet'))).toBe('configSet({"enabled":true,"projects":["C:\\\\work\\\\billing","C:\\\\work\\\\web"]})');
    expect(onSaved).toHaveBeenCalled();
  });

  it('Leave this team asks first', async () => {
    const { api } = await mount(teamConfig());
    await click(buttonByText(host, 'Leave this team'));
    expect(sheet()!.textContent).toContain('Leave this team?');
    expect(api.calls).not.toContain('leave()');
    await click(sheetButton('Leave'));
    await settle();
    expect(api.calls).toContain('leave()');
  });
});

describe('on a team as a member', () => {
  it('has no Invite, Remove or request buttons', async () => {
    await mount(
      teamConfig({
        admin: false,
        joins: [],
        members: [
          { id: 'm-leo', name: 'Leo', roles: ['admin'], admin: true, me: false, github: null, githubConfirmed: false, devices: 1 },
          { id: 'm-me', name: 'Maya', roles: ['approve'], admin: false, me: true, github: null, githubConfirmed: false, devices: 1 },
        ],
      }),
    );
    expect(hasText(host, 'Acme · 2 people')).toBe(true);
    expect(buttonByText(host, 'Invite someone')).toBeUndefined();
    expect(buttonByText(host, 'Remove')).toBeUndefined();
    expect(buttonByText(host, 'Leave this team')).toBeDefined();
  });
});
