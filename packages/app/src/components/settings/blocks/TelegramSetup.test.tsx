// @vitest-environment jsdom
// Settings > Phone > Telegram (early): the token is never shown back, pairing needs the desktop click, the bot
// stays silent until it is turned on, and Disconnect asks first. The API is injected; nothing calls Telegram.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { TelegramStatus } from '../../../lib/telegram-api';
import { click, makeEnv, settle, type as typeInto } from '../test-utils';
import {
  agoText,
  DISCONNECT_BODY,
  ERROR_TEXT,
  NO_STORE,
  statusLine,
  TELEGRAM_FOOTER,
  TELEGRAM_WARNING,
  TelegramSetup,
  type TelegramApi,
} from './TelegramSetup';
import { countdownText, nameOf } from './TelegramPairing';

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

const BASE: TelegramStatus = {
  storeAvailable: true,
  storeLabel: 'Windows Credential Manager',
  tokenSet: false,
  botUsername: null,
  enabled: false,
  state: 'off',
  error: null,
  paired: null,
  pairing: null,
  caps: ['watch', 'answer', 'type'],
  notifyRuns: false,
  shortMode: false,
  lastPollAtMs: null,
  strangersLastHour: 0,
};

const PAIRED = { username: 'anna_k', firstName: 'Anna', pairedAtMs: 1 };
const SAVED_TOKEN = { ...BASE, tokenSet: true, botUsername: 'anna_pm_bot' };
const PAIRED_STATUS: TelegramStatus = { ...SAVED_TOKEN, paired: PAIRED };

function fakeApi(start: TelegramStatus = BASE) {
  return {
    status: vi.fn(async () => start),
    setToken: vi.fn(async (_token: string) => start),
    pairStart: vi.fn(async () => start),
    pairDecide: vi.fn(async (_accept: boolean) => start),
    setEnabled: vi.fn(async (_on: boolean) => start),
    setOptions: vi.fn(async (_caps: string[], _notifyRuns: boolean, _shortMode: boolean) => start),
    forget: vi.fn(async () => start),
    onChanged: vi.fn((_listener: () => void) => () => {}),
  };
}
type FakeApi = ReturnType<typeof fakeApi>;

async function mount(api: FakeApi, env = makeEnv()) {
  await act(async () => {
    root.render(<TelegramSetup env={env} api={api as unknown as TelegramApi} />);
  });
  await settle();
  return env;
}

const text = () => host.textContent ?? '';
const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === label) ?? null;
const sw = (label: string) => host.querySelector<HTMLButtonElement>(`button[role="switch"][aria-label="${label}"]`);
const tokenField = () => host.querySelector<HTMLInputElement>('input[aria-label="Bot token"]');
const alertText = () => host.querySelector('[role="alert"]')?.textContent ?? null;

describe('TelegramSetup', () => {
  it('always shows the plain warning, even before the status has loaded', async () => {
    const api = fakeApi();
    api.status.mockReturnValue(new Promise(() => {}));
    await mount(api);
    expect(text()).toContain(TELEGRAM_WARNING);
    expect(text()).toContain("Telegram is a third party.");
  });

  it('shows the password field empty and never fills it with a saved token', async () => {
    const api = fakeApi(SAVED_TOKEN);
    await mount(api);
    expect(tokenField()?.type).toBe('password');
    expect(tokenField()?.value).toBe('');
    expect(text()).toContain('@anna_pm_bot ✓ · kept in Windows Credential Manager');
  });

  it('keeps Check off until a token is typed', async () => {
    await mount(fakeApi());
    expect(button('Check')?.disabled).toBe(true);
  });

  it('Check sends the token, clears the field and shows the bot and where it is kept', async () => {
    const api = fakeApi();
    api.setToken.mockResolvedValue(SAVED_TOKEN);
    const env = await mount(api);
    await typeInto(tokenField()!, '  123456789:TESTtokenTESTtokenTESTtokenTESTtok  ');
    await click(button('Check'));
    expect(api.setToken).toHaveBeenCalledWith('123456789:TESTtokenTESTtokenTESTtokenTESTtok');
    expect(tokenField()?.value).toBe('');
    expect(text()).toContain('@anna_pm_bot ✓ · kept in Windows Credential Manager');
    expect(env.markSaved).toHaveBeenCalled();
  });

  it('clears the token field even when Telegram refuses the token, and shows the reason', async () => {
    const api = fakeApi();
    api.setToken.mockRejectedValue(new Error('Telegram did not accept this token'));
    await mount(api);
    await typeInto(tokenField()!, '123456789:TESTtokenTESTtokenTESTtokenTESTtok');
    await click(button('Check'));
    expect(tokenField()?.value).toBe('');
    expect(alertText()).toBe('Telegram did not accept this token');
  });

  it('a store that is not available disables step 1 and says why', async () => {
    await mount(fakeApi({ ...BASE, storeAvailable: false }));
    expect(tokenField()?.disabled).toBe(true);
    expect(button('Check')?.disabled).toBe(true);
    expect(alertText()).toBe(NO_STORE);
  });

  it('shows step 2 only once a token is saved', async () => {
    await mount(fakeApi());
    expect(text()).not.toContain('Step 2 · Connect your chat');
    expect(button('Make a code')).toBeNull();
  });

  it('Make a code shows the code, the bot to message and a countdown', async () => {
    const api = fakeApi(SAVED_TOKEN);
    api.pairStart.mockResolvedValue({
      ...SAVED_TOKEN,
      pairing: { code: 'PM7K2QX9', expiresAtMs: Date.now() + 570_000, waitingFor: null },
    });
    await mount(api);
    await click(button('Make a code'));
    expect(api.pairStart).toHaveBeenCalledTimes(1);
    expect(host.querySelector('[aria-label="Pairing code"]')?.textContent).toBe('PM7K2QX9');
    expect(text()).toContain('Send /start PM7K2QX9 to @anna_pm_bot');
    expect(text()).toContain('Code ends in 9:30');
  });

  it('an expired code offers a new one', async () => {
    const api = fakeApi({ ...SAVED_TOKEN, pairing: { code: 'OLDCODE1', expiresAtMs: Date.now() - 1000, waitingFor: null } });
    await mount(api);
    expect(text()).toContain('This code ran out');
    expect(button('Make a new code')).not.toBeNull();
    expect(text()).not.toContain('OLDCODE1');
  });

  it('asks "Is this you?" and Yes, that is me calls pairDecide(true)', async () => {
    const api = fakeApi({
      ...SAVED_TOKEN,
      pairing: { code: 'PM7K2QX9', expiresAtMs: Date.now() + 570_000, waitingFor: { username: 'anna_k', firstName: 'Anna' } },
    });
    api.pairDecide.mockResolvedValue({ ...SAVED_TOKEN, paired: PAIRED });
    await mount(api);
    expect(text()).toContain('Is this you?');
    expect(text()).toContain('Telegram: Anna (@anna_k)');
    await click(button("Yes, that's me"));
    expect(api.pairDecide).toHaveBeenCalledWith(true);
    expect(text()).toContain('Connected to Anna (@anna_k).');
  });

  it('Not me calls pairDecide(false) and pairs nothing', async () => {
    const api = fakeApi({
      ...SAVED_TOKEN,
      pairing: { code: 'PM7K2QX9', expiresAtMs: Date.now() + 570_000, waitingFor: { username: null, firstName: 'Mallory' } },
    });
    await mount(api);
    await click(button('Not me'));
    expect(api.pairDecide).toHaveBeenCalledWith(false);
    expect(api.pairDecide).toHaveBeenCalledTimes(1);
  });

  it('Disconnect asks first and forgets nothing on No', async () => {
    const api = fakeApi(PAIRED_STATUS);
    const env = makeEnv();
    env.confirm.mockResolvedValueOnce(false);
    await mount(api, env);
    await click(button('Disconnect'));
    expect(env.confirm).toHaveBeenCalledWith(expect.objectContaining({ title: 'Disconnect Telegram?', body: DISCONNECT_BODY }));
    expect(api.forget).not.toHaveBeenCalled();
  });

  it('Disconnect forgets the token and the chat after a yes', async () => {
    const api = fakeApi(PAIRED_STATUS);
    const env = makeEnv();
    await mount(api, env);
    await click(button('Disconnect'));
    expect(api.forget).toHaveBeenCalledTimes(1);
  });

  it('turning Telegram on saves enabled=true', async () => {
    const api = fakeApi(PAIRED_STATUS);
    await mount(api);
    expect(sw('Telegram on')?.getAttribute('aria-checked')).toBe('false');
    await click(sw('Telegram on'));
    expect(api.setEnabled).toHaveBeenCalledWith(true);
  });

  it('offers three abilities and never control', async () => {
    const api = fakeApi(PAIRED_STATUS);
    await mount(api);
    expect(sw('See status and replies')).not.toBeNull();
    expect(sw('Answer questions')).not.toBeNull();
    expect(sw('Send messages to agents')).not.toBeNull();
    const labels = [...host.querySelectorAll('button[role="switch"]')].map((b) => b.getAttribute('aria-label') ?? '');
    expect(labels.join('|').toLowerCase()).not.toContain('control');
    expect(labels.join('|').toLowerCase()).not.toContain('start tasks');
  });

  it('turning an ability off saves the other two', async () => {
    const api = fakeApi(PAIRED_STATUS);
    await mount(api);
    expect(sw('Answer questions')?.getAttribute('aria-checked')).toBe('true');
    await click(sw('Answer questions'));
    expect(api.setOptions).toHaveBeenCalledWith(['watch', 'type'], false, false);
  });

  it('short messages only saves short_mode and keeps the rest', async () => {
    const api = fakeApi(PAIRED_STATUS);
    await mount(api);
    await click(sw('Short messages only (no agent text)'));
    expect(api.setOptions).toHaveBeenCalledWith(['watch', 'answer', 'type'], false, true);
  });

  it('tell me when a run finishes saves notify_runs', async () => {
    const api = fakeApi(PAIRED_STATUS);
    await mount(api);
    await click(sw('Tell me when a run finishes'));
    expect(api.setOptions).toHaveBeenCalledWith(['watch', 'answer', 'type'], true, false);
  });

  it('shows the on line with how long ago the last check was', async () => {
    const api = fakeApi({ ...PAIRED_STATUS, enabled: true, state: 'on', lastPollAtMs: Date.now() - 12_000 });
    await mount(api);
    expect(text()).toContain('On · last check 12 s ago');
  });

  it('shows the BotFather sentence for a conflict', async () => {
    const api = fakeApi({ ...PAIRED_STATUS, enabled: true, state: 'error', error: 'conflict' });
    await mount(api);
    expect(text()).toContain(ERROR_TEXT.conflict);
  });

  it('no stranger warning below 50 an hour', async () => {
    const quiet = fakeApi({ ...PAIRED_STATUS, strangersLastHour: 49 });
    await mount(quiet);
    expect(text()).not.toContain('Strangers are writing to your bot.');
  });

  it('shows the stranger warning at 50 an hour', async () => {
    await mount(fakeApi({ ...PAIRED_STATUS, strangersLastHour: 50 }));
    expect(text()).toContain("Strangers are writing to your bot. If you didn't share it, make a new token.");
  });

  it('keeps the footer: the bot is silent when the PC sleeps', async () => {
    await mount(fakeApi());
    expect(text()).toContain(TELEGRAM_FOOTER);
    expect(text()).toContain('See Telegram actions in Security > Audit.');
  });

  it('shows a refusal from the API as it is', async () => {
    const api = fakeApi(PAIRED_STATUS);
    api.setOptions.mockRejectedValue(new Error('Telegram is not allowed to do that.'));
    await mount(api);
    await click(sw('Short messages only (no agent text)'));
    expect(alertText()).toBe('Telegram is not allowed to do that.');
  });
});

describe('TelegramPairing helpers', () => {
  it('counts the code down and never goes below 0:00', () => {
    expect(countdownText(1_000 + 570_000, 1_000)).toBe('9:30');
    expect(countdownText(1_000 + 59_200, 1_000)).toBe('1:00');
    expect(countdownText(1_000, 5_000)).toBe('0:00');
  });

  it('names the person as Telegram shows them', () => {
    expect(nameOf({ username: 'anna_k', firstName: 'Anna' })).toBe('Anna (@anna_k)');
    expect(nameOf({ username: null, firstName: 'Anna' })).toBe('Anna');
    expect(nameOf({ username: null, firstName: null })).toBe('someone');
  });
});

describe('status line', () => {
  const now = 100_000;
  it('reads off, connecting, on and the error sentence', () => {
    expect(statusLine(BASE, now)).toBe('Off');
    expect(statusLine({ ...BASE, state: 'connecting' }, now)).toBe('Connecting…');
    expect(statusLine({ ...BASE, state: 'on', lastPollAtMs: now - 12_000 }, now)).toBe('On · last check 12 s ago');
    expect(statusLine({ ...BASE, state: 'error', error: 'token_rejected' }, now)).toBe(ERROR_TEXT.token_rejected);
  });

  it('shows minutes after a minute', () => {
    expect(agoText(3 * 60_000 + 4_000)).toBe('3 min ago');
  });
});
