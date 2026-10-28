// Settings > Phone > Telegram (early). Step 1 saves the bot token in the OS credential store; the app never
// shows it again, not even its last digits. Step 2 pairs one chat. Then the person turns the bot on and
// picks what it may do. Telegram is a third party, so the warning at the top always shows.
import { useEffect, useState, type KeyboardEvent } from 'react';
import {
  onTelegramChanged,
  telegramForget,
  telegramPairDecide,
  telegramPairStart,
  telegramSetEnabled,
  telegramSetOptions,
  telegramSetToken,
  telegramStatus,
  type TelegramCap,
  type TelegramStatus,
} from '../../../lib/telegram-api';
import type { SettingsEnv } from '../model';
import { Btn, FieldInput, Note, SettingBlock, SettingRow, SettingToggle } from '../settings-ui';
import { nameOf, TelegramPairing } from './TelegramPairing';

export const TELEGRAM_WARNING =
  "Telegram is a third party. Everything this bot sends and receives is stored on Telegram's servers and is not end-to-end encrypted. Puppet Master hides secrets, emails and folder names, but agent questions and replies are sent as text. Don't use this for work you must keep private.";

export const TELEGRAM_FOOTER = 'When your PC sleeps or Puppet Master is closed, the bot is silent.';
export const TELEGRAM_AUDIT = 'See Telegram actions in Security > Audit.';
export const NO_STORE = "This computer has no safe place for the token, so Telegram can't be used.";
export const DISCONNECT_BODY = 'Forget the bot token and the chat? You can connect again later.';

/** At this many strangers an hour the warning shows. */
export const STRANGERS_WARN_AT = 50;

/** What each capability lets the bot do. `control` is not offered: starting agents stays on this PC. */
export const TELEGRAM_CAPS: { cap: TelegramCap; label: string }[] = [
  { cap: 'watch', label: 'See status and replies' },
  { cap: 'answer', label: 'Answer questions' },
  { cap: 'type', label: 'Send messages to agents' },
];

export const ERROR_TEXT: Record<NonNullable<TelegramStatus['error']>, string> = {
  token_rejected: 'Telegram no longer accepts this token. Paste a new one.',
  conflict: 'Another program is using this bot. Make a new token in BotFather (/revoke) and paste it here.',
  webhook_set: 'This bot already sends its messages to a website. Make a new bot for Puppet Master.',
  blocked: "The bot can't write to you. Did you block it?",
  network: "Can't reach Telegram right now. Puppet Master keeps trying.",
};

/** "12 s" or "3 min": how long ago the last check was. */
export function agoText(ms: number): string {
  const secs = Math.max(0, Math.floor(ms / 1000));
  return secs < 60 ? `${secs} s ago` : `${Math.floor(secs / 60)} min ago`;
}

/** The one status line: the error sentence first, then the state. */
export function statusLine(s: TelegramStatus, now: number): string {
  if (s.error) return ERROR_TEXT[s.error];
  if (s.state === 'on') return s.lastPollAtMs == null ? 'On' : `On · last check ${agoText(now - s.lastPollAtMs)}`;
  if (s.state === 'connecting') return 'Connecting…';
  if (s.state === 'error') return 'Stopped. Turn it on again to retry.';
  return 'Off';
}

/** The seam the block talks through: the Tauri wrappers in production, a fake in tests. */
export interface TelegramApi {
  status: () => Promise<TelegramStatus>;
  setToken: (token: string) => Promise<TelegramStatus>;
  pairStart: () => Promise<TelegramStatus>;
  pairDecide: (accept: boolean) => Promise<TelegramStatus>;
  setEnabled: (on: boolean) => Promise<TelegramStatus>;
  setOptions: (caps: TelegramCap[], notifyRuns: boolean, shortMode: boolean) => Promise<TelegramStatus>;
  forget: () => Promise<TelegramStatus>;
  onChanged: (listener: () => void) => () => void;
}

export const telegramApi: TelegramApi = {
  status: telegramStatus,
  setToken: telegramSetToken,
  pairStart: telegramPairStart,
  pairDecide: telegramPairDecide,
  setEnabled: telegramSetEnabled,
  setOptions: telegramSetOptions,
  forget: telegramForget,
  onChanged: onTelegramChanged,
};

const errText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

export function TelegramSetup({ env, api = telegramApi }: { env: SettingsEnv; api?: TelegramApi }) {
  const [status, setStatus] = useState<TelegramStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [draft, setDraft] = useState('');
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    let live = true;
    const load = () => {
      api
        .status()
        .then((next) => live && setStatus(next))
        .catch((err) => live && setError(errText(err)));
    };
    load();
    const off = api.onChanged(load);
    const clock = setInterval(() => setNow(Date.now()), 1000);
    return () => {
      live = false;
      off();
      clearInterval(clock);
    };
  }, [api]);

  /** Runs one change and shows what came back. A refusal stays on screen as the reason. */
  const change = async (action: () => Promise<TelegramStatus>, saved = true) => {
    setError(null);
    setBusy(true);
    try {
      setStatus(await action());
      if (saved) env.markSaved();
    } catch (err) {
      setError(errText(err));
    } finally {
      setBusy(false);
    }
  };

  // The token leaves the field the moment Check is pressed. It is never kept on screen.
  const check = () => {
    const token = draft.trim();
    setDraft('');
    if (token) void change(() => api.setToken(token));
  };

  const disconnect = async () => {
    const ok = await env.confirm({ title: 'Disconnect Telegram?', body: DISCONNECT_BODY, confirmLabel: 'Disconnect' });
    if (ok) void change(() => api.forget());
  };

  const s = status;
  const saveOptions = (next: { caps?: TelegramCap[]; notifyRuns?: boolean; shortMode?: boolean }) => {
    if (!s) return;
    void change(() =>
      api.setOptions(next.caps ?? s.caps, next.notifyRuns ?? s.notifyRuns, next.shortMode ?? s.shortMode),
    );
  };
  const toggleCap = (cap: TelegramCap, on: boolean) => {
    if (!s) return;
    saveOptions({ caps: TELEGRAM_CAPS.map((c) => c.cap).filter((c) => (c === cap ? on : s.caps.includes(c))) });
  };

  return (
    <>
      <div className="st-tg-warn">{TELEGRAM_WARNING}</div>
      {error && <Note role="alert">{error}</Note>}
      {s && (
        <>
          <SettingBlock label="Step 1 · Make a bot" description="In Telegram, open @BotFather, send /newbot, pick a name, copy the token.">
            <div className="st-tg-token">
              <FieldInput
                aria-label="Bot token"
                type="password"
                autoComplete="off"
                spellCheck={false}
                disabled={!s.storeAvailable || busy}
                value={draft}
                placeholder={s.tokenSet ? 'Paste a new token to replace it' : 'Paste the token here'}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e: KeyboardEvent<HTMLInputElement>) => {
                  if (e.key === 'Enter') check();
                }}
              />
              <Btn small disabled={!s.storeAvailable || busy || !draft.trim()} onClick={check}>
                Check
              </Btn>
            </div>
            {!s.storeAvailable ? (
              <Note role="alert">{NO_STORE}</Note>
            ) : (
              s.tokenSet && <Note role="status">{`${s.botUsername ? `@${s.botUsername} ✓` : 'Token saved'} · kept in ${s.storeLabel}`}</Note>
            )}
          </SettingBlock>

          {s.tokenSet && (
            <SettingBlock
              label="Step 2 · Connect your chat"
              description={s.paired ? 'Only this chat can talk to Puppet Master.' : 'Make a code and send it to your bot. Only that chat can talk to Puppet Master.'}
            >
              {s.paired ? (
                <Note role="status">{`Connected to ${nameOf(s.paired)}.`}</Note>
              ) : (
                <TelegramPairing
                  status={s}
                  now={now}
                  busy={busy}
                  onMakeCode={() => void change(() => api.pairStart(), false)}
                  onDecide={(accept) => void change(() => api.pairDecide(accept))}
                />
              )}
            </SettingBlock>
          )}

          {s.paired && (
            <>
              <SettingToggle
                label="Telegram on"
                description={statusLine(s, now)}
                checked={s.enabled}
                disabled={busy}
                onChange={(on) => void change(() => api.setEnabled(on))}
              />
              {s.strangersLastHour >= STRANGERS_WARN_AT && (
                <Note role="alert">Strangers are writing to your bot. If you didn't share it, make a new token.</Note>
              )}
              <SettingBlock label="What Telegram may do" description="Starting and stopping agents and settings stay on this PC." />
              {TELEGRAM_CAPS.map(({ cap, label }) => (
                <SettingToggle
                  key={cap}
                  label={label}
                  checked={s.caps.includes(cap)}
                  disabled={busy}
                  onChange={(on) => toggleCap(cap, on)}
                />
              ))}
              <SettingToggle
                label="Tell me when a run finishes"
                description="Also sends a message when a run ends or fails."
                checked={s.notifyRuns}
                disabled={busy}
                onChange={(on) => saveOptions({ notifyRuns: on })}
              />
              <SettingToggle
                label="Short messages only (no agent text)"
                description={'Only "needs you" messages. No agent text is sent.'}
                checked={s.shortMode}
                disabled={busy}
                onChange={(on) => saveOptions({ shortMode: on })}
              />
            </>
          )}

          {(s.tokenSet || s.paired) && (
            <SettingRow label="Disconnect" description="Forget the bot token and the chat.">
              <Btn small disabled={busy} onClick={() => void disconnect()}>
                Disconnect
              </Btn>
            </SettingRow>
          )}
          <Note>{`${TELEGRAM_FOOTER} ${TELEGRAM_AUDIT}`}</Note>
        </>
      )}
    </>
  );
}
