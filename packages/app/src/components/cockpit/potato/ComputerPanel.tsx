// The top of the Computer tab: a live preview card of the bot's screen (view only), the "<Bot>'s screen" row with Take
// control, and who has the computer ("Not in use", "<Bot> is using it", "You have control", "Needs you" and why). Take
// control makes the person the one in charge and opens the full-window computer (ComputerOverlay); I'm done, Esc and Close
// there hand it back. Nothing is drawn while the Labs switches are off (the app says so in the screen state): `whenOff`
// then explains how to turn it on. The screen password lives only in the iframe address (see useScreenLink). No colour.
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import type { AvatarId } from '../../../lib/companion';
import { plainError } from '../../../lib/potato';
import { COMPUTER_COPY } from '../../../lib/potato-computer';
import type { SandboxState } from '../../../lib/potato-sandbox';
import { headerWords, type ScreenState, type ScreenTransport } from '../../../lib/potatoScreen';
import { ComputerOverlay } from './ComputerOverlay';
import { ComputerPreview } from './ComputerPreview';
import { StatusPill } from './ui/StatusPill';
import '../../../styles/cockpit-potato-panel.css';

export { KEEP_EVERY_MS, SILENT_VIEWER_MS } from './useScreenLink';

export interface ComputerPanelProps {
  bot: { id: string; name: string };
  avatar?: AvatarId;
  transport: ScreenTransport;
  /** Said when a button fails. */
  onNotify?: (text: string) => void;
  /** Drawn instead of nothing while the Labs switches are off (the Computer tab explains itself). */
  whenOff?: ReactNode;
  /** The sandbox's own state, when something reads it: an asleep computer shows "Wake" instead of a screen. */
  sandboxState?: SandboxState | null;
  onWake?: () => void;
  waking?: boolean;
  /** Drawn instead of the preview and the rows (the computer is not set up yet). */
  notSetUp?: ReactNode;
  /** Drawn under them (the maintenance section). */
  after?: ReactNode;
}

export function ComputerPanel({ bot, avatar, transport, onNotify, whenOff = null, sandboxState = null, onWake, waking, notSetUp, after }: ComputerPanelProps) {
  const [screen, setScreen] = useState<ScreenState | null>(null);
  const [full, setFull] = useState(false);
  const [busy, setBusy] = useState(false);
  const takeRef = useRef<HTMLButtonElement>(null);
  const hadControl = useRef(false);

  const enabled = screen?.enabled === true;
  const phase = screen?.phase ?? 'free';

  const reload = useCallback(() => {
    void transport.state().then(setScreen, () => setScreen(null));
  }, [transport]);

  // The state: read now, and again whenever the app says it moved.
  useEffect(() => {
    reload();
    let off: (() => void) | undefined;
    let gone = false;
    void transport.onChanged?.(reload).then((unlisten) => {
      if (gone) unlisten();
      else off = unlisten;
    });
    return () => {
      gone = true;
      off?.();
    };
  }, [transport, reload]);

  // The person lost the screen some other way (the lease ran out, they handed it back elsewhere): the full window goes.
  useEffect(() => {
    if (hadControl.current && phase !== 'you') setFull(false);
    hadControl.current = phase === 'you';
  }, [phase]);
  // The switch is turned off while the window is open.
  useEffect(() => {
    if (screen && !enabled) setFull(false);
  }, [screen, enabled]);

  // The full window closed: the keyboard goes back to the button that opened it.
  const wasFull = useRef(false);
  useEffect(() => {
    if (wasFull.current && !full) takeRef.current?.focus();
    wasFull.current = full;
  }, [full]);

  const call = useCallback(
    async (run: () => Promise<ScreenState>): Promise<boolean> => {
      setBusy(true);
      try {
        setScreen(await run());
        return true;
      } catch (error) {
        onNotify?.(plainError(error, 'That did not work. Try again.'));
        reload();
        return false;
      } finally {
        setBusy(false);
      }
    },
    [onNotify, reload],
  );

  const takeControl = async () => {
    if (busy) return;
    if (await call(() => transport.takeOver())) setFull(true);
  };
  const skip = (cardId: string) => {
    if (!busy) void call(() => transport.skip(cardId));
  };
  const handBack = async () => {
    if (busy) return false;
    return call(() => transport.handBack());
  };

  if (!enabled || !screen) return <>{whenOff}</>;

  const asking = phase === 'needs_you';
  const awake = sandboxState === null || sandboxState === 'running';
  const words = headerWords(screen, bot.id, bot.name);

  return (
    <div className="ck-potato-computer-wrap">
      {notSetUp ?? (
        <section className="ck-potato-computer" aria-label="Computer">
          <ComputerPreview
            bot={bot}
            transport={transport}
            sandboxState={sandboxState}
            inFullWindow={full}
            onOpen={() => setFull(true)}
            onWake={onWake}
            waking={waking}
          />
          <div className="ck-potato-computer-row">
            <h2 className="ck-potato-computer-label">{COMPUTER_COPY.screenOf(bot.name)}</h2>
            <div className="ck-potato-computer-actions">
              {asking && screen.cardId && (
                <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={() => skip(screen.cardId!)}>
                  {COMPUTER_COPY.skip}
                </button>
              )}
              {phase === 'you' ? (
                <>
                  <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={() => setFull(true)}>
                    {COMPUTER_COPY.open}
                  </button>
                  <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={() => void handBack()}>
                    {COMPUTER_COPY.imDone}
                  </button>
                </>
              ) : (
                <button
                  ref={takeRef}
                  type="button"
                  className="ck-btn ck-btn--secondary"
                  disabled={busy || !awake}
                  title={awake ? undefined : COMPUTER_COPY.wakeFirst}
                  onClick={() => void takeControl()}
                >
                  {COMPUTER_COPY.takeControl}
                </button>
              )}
            </div>
          </div>
          <div className="ck-potato-computer-status">
            <p className={`ck-potato-computer-state is-${phase}`} role="status">
              {asking && screen.bot === bot.id ? <StatusPill tone="strong">{words}</StatusPill> : phase === 'you' ? <StatusPill tone="quiet">{words}</StatusPill> : words}
            </p>
            {asking && screen.reason && <p className="ck-sub ck-potato-computer-reason">{screen.reason}</p>}
          </div>
        </section>
      )}
      {after}
      {full && (
        <ComputerOverlay
          bot={bot}
          avatar={avatar}
          transport={transport}
          screen={screen}
          busy={busy}
          onTakeControl={() => void takeControl()}
          onSkip={skip}
          onHandBack={handBack}
          onClose={() => setFull(false)}
        />
      )}
    </div>
  );
}
