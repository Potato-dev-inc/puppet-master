// @vitest-environment jsdom
// Settings > Phone > Telegram > Tag Your Agent in groups: off until switched on, a group is approved only by a
// click here, who may tag / agent / folder / style are saved per group, and Remove asks first. The API is
// injected; nothing calls Telegram.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { TagBotView, TagGroup } from '../../../lib/telegram-api';
import { blur, click, makeEnv, pickOption, settle, type as typeInto } from '../test-utils';
import { RECENT_SHOWN, TAG_ANYONE_FOLDER_WARNING, TAG_PRIVACY, TAG_REMOVE_BODY, TAG_TITLE, TelegramTagBot, type TagBotApi } from './TelegramTagBot';

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

const CREW = -100555000001;
const OTHER = -100555000002;

const GROUP: TagGroup = { chatId: CREW, title: 'Test Crew', who: 'only_me', agent: null, folder: null, style: '' };

const OFF: TagBotView = {
  paired: true,
  botUsername: 'anna_pm_bot',
  enabled: false,
  dailyCap: 30,
  perPersonCap: 5,
  groups: [],
  pending: [],
  agents: [
    { id: 'test-agent', label: 'Test Agent' },
    { id: 'other-agent', label: 'Other Agent' },
  ],
  recent: [],
};
const ON: TagBotView = { ...OFF, enabled: true };
const WITH_PENDING: TagBotView = { ...ON, pending: [{ chatId: OTHER, title: 'New Group', seenAtMs: 5 }] };
const WITH_GROUP: TagBotView = { ...ON, groups: [GROUP] };

function fakeApi(start: TagBotView = OFF) {
  return {
    view: vi.fn(async () => start),
    setEnabled: vi.fn(async (_on: boolean) => start),
    setCaps: vi.fn(async (_daily: number, _person: number) => start),
    approve: vi.fn(async (_id: number) => start),
    ignore: vi.fn(async (_id: number) => start),
    updateGroup: vi.fn(async (_id: number, _update: unknown) => start),
    remove: vi.fn(async (_id: number) => start),
    onChanged: vi.fn((_listener: () => void) => () => {}),
  };
}
type FakeApi = ReturnType<typeof fakeApi>;

async function mount(api: FakeApi, env = makeEnv()) {
  await act(async () => {
    root.render(<TelegramTagBot env={env} api={api as unknown as TagBotApi} />);
  });
  await settle();
  return env;
}

const text = () => host.textContent ?? '';
const button = (label: string) => host.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`) ?? [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === label) ?? null;
const sw = (label: string) => host.querySelector<HTMLButtonElement>(`button[role="switch"][aria-label="${label}"]`);
const field = (label: string) => host.querySelector<HTMLInputElement>(`input[aria-label="${label}"]`);
const select = (label: string) => host.querySelector<HTMLSelectElement>(`select[aria-label="${label}"]`);

describe('TelegramTagBot', () => {
  it('shows nothing until a chat is paired, so there is nothing to switch on yet', async () => {
    await mount(fakeApi({ ...OFF, paired: false }));
    expect(text()).toBe('');
    expect(sw(TAG_TITLE)).toBeNull();
  });

  it('is off by default and says to keep privacy mode on', async () => {
    await mount(fakeApi());
    expect(sw(TAG_TITLE)?.getAttribute('aria-checked')).toBe('false');
    expect(text()).toContain(TAG_PRIVACY);
    expect(text()).toContain('Keep Telegram privacy mode on — the bot then only sees messages that tag it.');
    // While it is off, the group lists and the caps are not offered.
    expect(text()).not.toContain('Approved groups');
    expect(text()).not.toContain('Answers a day');
  });

  it('the switch turns it on and then shows groups, caps and recent answers', async () => {
    const api = fakeApi();
    api.setEnabled.mockResolvedValue(ON);
    const env = await mount(api);
    await click(sw(TAG_TITLE));
    expect(api.setEnabled).toHaveBeenCalledWith(true);
    expect(env.markSaved).toHaveBeenCalled();
    expect(text()).toContain('Groups waiting for your yes');
    expect(text()).toContain('Approved groups');
    expect(text()).toContain('No groups yet.');
    expect(text()).toContain('Answers a day');
    expect(text()).toContain('Answers an hour for one person');
    expect(text()).toContain('Nothing yet.');
  });

  it('a pending group waits for Approve; Approve and Ignore call the PC with its id', async () => {
    const api = fakeApi(WITH_PENDING);
    await mount(api);
    expect(text()).toContain('New Group');
    expect(api.approve).not.toHaveBeenCalled();
    await click(button('Approve New Group'));
    expect(api.approve).toHaveBeenCalledWith(OTHER);
    await click(button('Ignore New Group'));
    expect(api.ignore).toHaveBeenCalledWith(OTHER);
  });

  it('shows a reason when the PC refuses', async () => {
    const api = fakeApi(WITH_PENDING);
    api.approve.mockRejectedValue(new Error('You can approve 20 groups. Remove one first.'));
    await mount(api);
    await click(button('Approve New Group'));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('You can approve 20 groups. Remove one first.');
  });

  it('an approved group starts with Only me, any agent and no folder', async () => {
    await mount(fakeApi(WITH_GROUP));
    expect(select('Who may tag in Test Crew')?.value).toBe('only_me');
    expect(select('Agent for Test Crew')?.value).toBe('');
    expect(field('Folder for Test Crew')?.value).toBe('');
    expect(field('Style note for Test Crew')?.maxLength).toBe(200);
    expect(text()).not.toContain(TAG_ANYONE_FOLDER_WARNING);
  });

  it('who may tag and the agent save the moment they are picked, keeping the other fields', async () => {
    const api = fakeApi({ ...ON, groups: [{ ...GROUP, folder: 'D:\\work\\shop', style: 'brief' }] });
    await mount(api);
    await pickOption(select('Who may tag in Test Crew')!, 'anyone');
    expect(api.updateGroup).toHaveBeenLastCalledWith(CREW, { who: 'anyone', agent: null, folder: 'D:\\work\\shop', style: 'brief' });
    await pickOption(select('Agent for Test Crew')!, 'other-agent');
    expect(api.updateGroup).toHaveBeenLastCalledWith(CREW, { who: 'only_me', agent: 'other-agent', folder: 'D:\\work\\shop', style: 'brief' });
    await pickOption(select('Agent for Test Crew')!, '');
    expect(api.updateGroup).toHaveBeenLastCalledWith(CREW, { who: 'only_me', agent: null, folder: 'D:\\work\\shop', style: 'brief' });
  });

  it('the agent list comes from the PC, and an agent no longer installed stays selectable', async () => {
    await mount(fakeApi({ ...ON, groups: [{ ...GROUP, agent: 'gone-agent' }] }));
    const options = [...select('Agent for Test Crew')!.options].map((o) => o.value);
    expect(options).toEqual(['', 'test-agent', 'other-agent', 'gone-agent']);
    expect(select('Agent for Test Crew')?.value).toBe('gone-agent');
  });

  it('the folder and the style note save when the field is left, and empty means none', async () => {
    const api = fakeApi({ ...ON, groups: [{ ...GROUP, folder: 'D:\\work\\shop' }] });
    await mount(api);
    const folder = field('Folder for Test Crew')!;
    await typeInto(folder, '  D:\\work\\other  ');
    expect(api.updateGroup).not.toHaveBeenCalled();
    await blur(folder);
    expect(api.updateGroup).toHaveBeenLastCalledWith(CREW, { who: 'only_me', agent: null, folder: 'D:\\work\\other', style: '' });
    const style = field('Style note for Test Crew')!;
    await typeInto(style, 'short and friendly');
    await blur(style);
    expect(api.updateGroup).toHaveBeenLastCalledWith(CREW, { who: 'only_me', agent: null, folder: 'D:\\work\\shop', style: 'short and friendly' });
    await typeInto(folder, '');
    await blur(folder);
    expect(api.updateGroup).toHaveBeenLastCalledWith(CREW, { who: 'only_me', agent: null, folder: null, style: '' });
  });

  it('warns when anyone may tag in a group that has a folder, and only then', async () => {
    await mount(fakeApi({ ...ON, groups: [{ ...GROUP, who: 'anyone', folder: 'D:\\work\\shop' }] }));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe(TAG_ANYONE_FOLDER_WARNING);
    act(() => root.unmount());
    root = createRoot(host);
    await mount(fakeApi({ ...ON, groups: [{ ...GROUP, who: 'anyone', folder: null }] }));
    expect(text()).not.toContain(TAG_ANYONE_FOLDER_WARNING);
  });

  it('Remove asks first and only then removes the group', async () => {
    const api = fakeApi(WITH_GROUP);
    const env = await mount(api);
    env.confirm.mockResolvedValueOnce(false);
    await click(button('Remove Test Crew'));
    expect(env.confirm).toHaveBeenCalledWith({ title: 'Remove Test Crew?', body: TAG_REMOVE_BODY, confirmLabel: 'Remove' });
    expect(api.remove).not.toHaveBeenCalled();
    await click(button('Remove Test Crew'));
    expect(api.remove).toHaveBeenCalledWith(CREW);
  });

  it('the caps save with the other cap kept', async () => {
    const api = fakeApi(ON);
    await mount(api);
    const daily = field('Answers a day')!;
    await typeInto(daily, '12');
    await blur(daily);
    expect(api.setCaps).toHaveBeenLastCalledWith(12, 5);
    const person = field('Answers an hour for one person')!;
    await typeInto(person, '2');
    await blur(person);
    expect(api.setCaps).toHaveBeenLastCalledWith(30, 2);
  });

  it('lists the last answers, newest first, at most five, and says so when one failed', async () => {
    const recent = Array.from({ length: 7 }, (_, i) => ({
      atMs: 100 - i,
      groupTitle: 'Test Crew',
      asker: 'Sam',
      question: `question ${i}`,
      answer: i === 1 ? null : `answer ${i}`,
      agent: 'Test Agent',
      ok: i !== 1,
    }));
    await mount(fakeApi({ ...ON, recent }));
    const items = host.querySelectorAll('.st-tag-recent li');
    expect(items).toHaveLength(RECENT_SHOWN);
    expect(items[0].textContent).toContain('question 0');
    expect(items[0].textContent).toContain('answer 0');
    expect(items[1].textContent).toContain("Couldn't answer.");
    expect(text()).not.toContain('question 6');
  });

  it('reads the PC again when it says something changed', async () => {
    const api = fakeApi(ON);
    let listener: (() => void) | null = null;
    api.onChanged.mockImplementation((cb: () => void) => {
      listener = cb;
      return () => {};
    });
    await mount(api);
    expect(api.view).toHaveBeenCalledTimes(1);
    api.view.mockResolvedValue(WITH_PENDING);
    await act(async () => listener?.());
    await settle();
    expect(text()).toContain('New Group');
  });
});
