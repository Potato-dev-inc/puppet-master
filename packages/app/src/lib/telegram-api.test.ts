import { afterEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
const listen = vi.fn();
const unlisten = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: (...args: unknown[]) => listen(...args) }));

import {
  TELEGRAM_CHANGED_EVENT,
  onTelegramChanged,
  telegramForget,
  telegramPairDecide,
  telegramPairStart,
  telegramSetEnabled,
  telegramSetOptions,
  telegramSetToken,
  telegramStatus,
  type TelegramStatus,
} from './telegram-api';

const status: TelegramStatus = {
  storeAvailable: true,
  storeLabel: 'Windows Credential Manager',
  tokenSet: true,
  botUsername: 'anna_pm_bot',
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

function desktop(on: boolean) {
  if (on) {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  } else {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  }
}

afterEach(() => {
  invoke.mockReset();
  listen.mockReset();
  unlisten.mockReset();
  desktop(false);
});

describe('telegram commands in the desktop app', () => {
  it('reads the status with no arguments', async () => {
    desktop(true);
    invoke.mockResolvedValue(status);
    await expect(telegramStatus()).resolves.toEqual(status);
    expect(invoke).toHaveBeenCalledWith('telegram_status', undefined);
  });

  it('sends the token only to telegram_set_token, under the name the command expects', async () => {
    desktop(true);
    invoke.mockResolvedValue(status);
    await telegramSetToken('123456789:abc');
    expect(invoke).toHaveBeenCalledWith('telegram_set_token', { token: '123456789:abc' });
  });

  it('calls the pairing commands with the right argument', async () => {
    desktop(true);
    invoke.mockResolvedValue(status);
    await telegramPairStart();
    expect(invoke).toHaveBeenLastCalledWith('telegram_pair_start', undefined);
    await telegramPairDecide(true);
    expect(invoke).toHaveBeenLastCalledWith('telegram_pair_decide', { accept: true });
    await telegramPairDecide(false);
    expect(invoke).toHaveBeenLastCalledWith('telegram_pair_decide', { accept: false });
  });

  it('turns the chat on and off with telegram_set_enabled', async () => {
    desktop(true);
    invoke.mockResolvedValue(status);
    await telegramSetEnabled(true);
    expect(invoke).toHaveBeenLastCalledWith('telegram_set_enabled', { on: true });
    await telegramSetEnabled(false);
    expect(invoke).toHaveBeenLastCalledWith('telegram_set_enabled', { on: false });
  });

  it('sends the options in the camel case names the command takes', async () => {
    desktop(true);
    invoke.mockResolvedValue(status);
    await telegramSetOptions({ caps: ['watch', 'answer'], notifyRuns: true, shortMode: false });
    expect(invoke).toHaveBeenLastCalledWith('telegram_set_options', {
      caps: ['watch', 'answer'],
      notifyRuns: true,
      shortMode: false,
    });
  });

  it('forgets the token and chat with telegram_forget', async () => {
    desktop(true);
    invoke.mockResolvedValue(status);
    await telegramForget();
    expect(invoke).toHaveBeenLastCalledWith('telegram_forget', undefined);
  });

  it('refuses to send a control cap at compile time', () => {
    // @ts-expect-error `control` is never a Telegram cap; the Rust side refuses it too.
    const bad: Parameters<typeof telegramSetOptions>[0] = { caps: ['control'], notifyRuns: false, shortMode: false };
    expect(bad.caps).toEqual(['control']);
  });
});

describe('telegram commands outside the desktop app', () => {
  it('never calls invoke and says a desktop shell is needed', async () => {
    desktop(false);
    await expect(telegramStatus()).rejects.toThrow('Desktop shell required: telegram_status');
    await expect(telegramSetToken('x')).rejects.toThrow('Desktop shell required: telegram_set_token');
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe('onTelegramChanged', () => {
  it('listens to telegram://changed and calls back without a payload', async () => {
    desktop(true);
    listen.mockResolvedValue(unlisten);
    const cb = vi.fn();
    const stop = await onTelegramChanged(cb);
    expect(listen).toHaveBeenCalledWith(TELEGRAM_CHANGED_EVENT, expect.any(Function));
    expect(TELEGRAM_CHANGED_EVENT).toBe('telegram://changed');

    const handler = listen.mock.calls[0][1] as () => void;
    handler();
    expect(cb).toHaveBeenCalledTimes(1);
    expect(stop).toBe(unlisten);
  });

  it('does nothing outside the desktop app', async () => {
    desktop(false);
    const cb = vi.fn();
    const stop = await onTelegramChanged(cb);
    expect(listen).not.toHaveBeenCalled();
    expect(() => stop()).not.toThrow();
    expect(cb).not.toHaveBeenCalled();
  });
});
