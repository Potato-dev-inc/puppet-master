// Settings > Phone > Telegram, step 2: a one-time code the person sends to their bot, then the desktop
// "Is this you?" card. Both the code and the click are needed, so a stranger who guesses the code still
// cannot pair without the person saying yes here. Plain content only: the step's label comes from the caller.
import type { TelegramStatus } from '../../../lib/telegram-api';
import { Btn, Note } from '../settings-ui';

/** "9:30": the time left on the code, rounded up so it never shows 0:00 while the code still works. */
export function countdownText(expiresAtMs: number, now: number): string {
  const secs = Math.max(0, Math.ceil((expiresAtMs - now) / 1000));
  return `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, '0')}`;
}

/** "Anna (@anna_k)", or "someone" when Telegram sent no name. */
export function nameOf(who: { username: string | null; firstName: string | null }): string {
  const name = who.firstName?.trim() || 'someone';
  return who.username ? `${name} (@${who.username})` : name;
}

export function TelegramPairing({
  status,
  now,
  busy,
  onMakeCode,
  onDecide,
}: {
  status: TelegramStatus;
  now: number;
  busy: boolean;
  onMakeCode: () => void;
  onDecide: (accept: boolean) => void;
}) {
  const pairing = status.pairing;

  // Someone sent the code: the desktop asks first. Nothing pairs without this click.
  if (pairing?.waitingFor) {
    return (
      <div className="st-tg-pair">
        <p className="st-tg-ask">
          Is this you? <span>Telegram: {nameOf(pairing.waitingFor)}</span>
        </p>
        <div className="st-tg-decide">
          <Btn primary small disabled={busy} onClick={() => onDecide(true)}>
            Yes, that's me
          </Btn>
          <Btn small disabled={busy} onClick={() => onDecide(false)}>
            Not me
          </Btn>
        </div>
      </div>
    );
  }

  // No code yet, or the code ran out: make a new one.
  if (!pairing || now >= pairing.expiresAtMs) {
    return (
      <div className="st-tg-pair">
        {pairing ? (
          <Note role="status">This code ran out. Codes last 10 minutes and work once.</Note>
        ) : (
          <Note>Puppet Master makes a code that works once, for 10 minutes.</Note>
        )}
        <div>
          <Btn primary small disabled={busy || !status.tokenSet} onClick={onMakeCode}>
            {pairing ? 'Make a new code' : 'Make a code'}
          </Btn>
        </div>
      </div>
    );
  }

  // The code is live: send it to the bot in a private chat.
  const bot = status.botUsername ? `@${status.botUsername}` : 'your bot';
  return (
    <div className="st-tg-pair">
      <div className="st-tg-code" aria-label="Pairing code">
        {pairing.code}
      </div>
      <Note>
        Send /start {pairing.code} to {bot}. Code ends in {countdownText(pairing.expiresAtMs, now)}.
      </Note>
      <Note role="status">Waiting for your message…</Note>
    </div>
  );
}
