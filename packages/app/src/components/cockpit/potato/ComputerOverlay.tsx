// The computer in the full window: a layer over the whole app window (a portal into the cockpit root) with a top bar (the
// bot, who has the screen, Skip when the bot asked, "I'm done" to hand back, Close) and the screen under it. The viewer is
// in control mode once the person has the screen, and in view mode while they only look. Esc and Close hand the screen
// back after one question; looking only just closes. Keys inside the screen belong to the screen (an Esc typed there
// goes to the bot's computer), so the keyboard keeps the lease alive while the screen has it.
import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import type { AvatarId } from '../../../lib/companion';
import { COMPUTER_COPY } from '../../../lib/potato-computer';
import { headerWords, type ScreenMode, type ScreenState, type ScreenTransport } from '../../../lib/potatoScreen';
import { Avatar } from '../../companion/Avatar';
import { useSheetKeys } from '../Sheet';
import { StatusPill, type PillTone } from './ui/StatusPill';
import { Glyph } from './ui/Glyph';
import { useScreenLink } from './useScreenLink';
import '../../../styles/cockpit-potato-overlay.css';

export interface ComputerOverlayProps {
  bot: { id: string; name: string };
  avatar?: AvatarId;
  transport: ScreenTransport;
  screen: ScreenState;
  /** A call (Take control, Skip, I'm done) is running. */
  busy: boolean;
  onTakeControl: () => void;
  onSkip: (cardId: string) => void;
  /** Hands the screen back; resolves when it is done (the overlay then closes). */
  onHandBack: () => Promise<boolean>;
  /** Closes the layer (the screen is not handed back by this). */
  onClose: () => void;
}

function toneOf(phase: ScreenState['phase']): PillTone | null {
  if (phase === 'needs_you' || phase === 'bot') return 'strong';
  if (phase === 'you') return 'quiet';
  return null;
}

export function ComputerOverlay({ bot, avatar, transport, screen, busy, onTakeControl, onSkip, onHandBack, onClose }: ComputerOverlayProps) {
  const [confirming, setConfirming] = useState(false);
  const mine = screen.phase === 'you';
  const mode: ScreenMode = mine ? 'control' : 'view';
  const view = useScreenLink({ transport, botId: bot.id, mode, active: true, keepLease: mine });
  const asking = screen.phase === 'needs_you' && screen.bot === bot.id;

  // Close, Esc: while the person holds the screen, ask once; otherwise just look away.
  const wantClose = () => {
    if (mine) setConfirming((v) => !v);
    else onClose();
  };
  const ref = useSheetKeys(() => {
    if (confirming) setConfirming(false);
    else wantClose();
  });
  useEffect(() => {
    // The keyboard starts on the layer itself, so Esc works at once.
    ref.current?.focus();
  }, [ref]);

  const handBack = async () => {
    const done = await onHandBack();
    if (done) onClose();
    setConfirming(false);
  };

  const tone = toneOf(screen.phase);
  const words = headerWords(screen, bot.id, bot.name);
  const scope = document.querySelector('.cockpit') ?? document.body;
  return createPortal(
    <div ref={ref} className="ck-potato-overlay" role="dialog" aria-modal="true" aria-label={COMPUTER_COPY.overlayTitle(bot.name)} tabIndex={-1}>
      <header className="ck-potato-overlay-bar">
        {avatar && <Avatar id={avatar} size={24} state={screen.phase === 'bot' ? 'working' : 'idle'} name={bot.name} />}
        <h2 className="ck-potato-overlay-title">{COMPUTER_COPY.overlayTitle(bot.name)}</h2>
        <p className="ck-potato-overlay-state" role="status">
          {tone ? <StatusPill tone={tone}>{words}</StatusPill> : <span className="ck-sub">{words}</span>}
          {view.title && <span className="ck-sub ck-potato-overlay-window">{view.title}</span>}
        </p>
        <span className="ck-potato-overlay-gap" />
        {asking && screen.cardId && (
          <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={() => onSkip(screen.cardId!)}>
            {COMPUTER_COPY.skip}
          </button>
        )}
        {screen.phase !== 'you' && (
          <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={onTakeControl}>
            {COMPUTER_COPY.takeControl}
          </button>
        )}
        {screen.phase === 'you' && (
          <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => void handBack()}>
            {COMPUTER_COPY.imDone}
          </button>
        )}
        <button type="button" className="ck-icon-btn" aria-label={COMPUTER_COPY.close} title={COMPUTER_COPY.close} onClick={wantClose}>
          <Glyph name="close" />
        </button>
      </header>
      {asking && screen.reason && <p className="ck-potato-overlay-reason">{screen.reason}</p>}
      {confirming && (
        <div className="ck-potato-overlay-confirm" role="alertdialog" aria-labelledby="ck-potato-overlay-confirm-title">
          <p id="ck-potato-overlay-confirm-title" className="ck-potato-overlay-confirm-title">
            {COMPUTER_COPY.confirmTitle(bot.name)}
          </p>
          <p className="ck-sub">{COMPUTER_COPY.confirmBody(bot.name)}</p>
          <div className="ck-potato-overlay-confirm-actions">
            <button type="button" className="ck-btn ck-btn--secondary" onClick={() => setConfirming(false)}>
              {COMPUTER_COPY.keepControl}
            </button>
            <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => void handBack()}>
              {COMPUTER_COPY.handBack}
            </button>
          </div>
        </div>
      )}
      <div className="ck-potato-overlay-body">
        {view.link && (
          <iframe
            key={`${view.link.mode}-${view.attempt}`}
            ref={view.frame}
            className="ck-potato-overlay-frame"
            title={`${bot.name}'s computer`}
            src={view.link.src}
            sandbox="allow-scripts allow-same-origin"
            allow="clipboard-read; clipboard-write"
          />
        )}
        {view.note && (
          <div className="ck-potato-computer-note" role="status">
            <span>{view.note}</span>
            {view.failed && (
              <button type="button" className="ck-btn ck-btn--secondary" onClick={view.retry}>
                {COMPUTER_COPY.retry}
              </button>
            )}
          </div>
        )}
      </div>
    </div>,
    scope,
  );
}
