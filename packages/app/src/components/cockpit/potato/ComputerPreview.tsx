// The live preview card of the Computer tab: the bot's screen in a 16:10 card, view only (the iframe takes no pointer, the
// whole card is one "Open in full window" button), with the title of the window the bot is on when the viewer says it. It
// also says when there is nothing to see: the computer is asleep or waking, or the screen is open in the full window.
import type { ScreenTransport } from '../../../lib/potatoScreen';
import { COMPUTER_COPY } from '../../../lib/potato-computer';
import type { SandboxState } from '../../../lib/potato-sandbox';
import { useScreenLink } from './useScreenLink';
import '../../../styles/cockpit-potato-panel.css';

export interface ComputerPreviewProps {
  bot: { id: string; name: string };
  transport: ScreenTransport;
  /** The sandbox's own state; null when nobody reads it (the screen is then tried). */
  sandboxState: SandboxState | null;
  /** The full window has the screen: the preview lets go of it. */
  inFullWindow: boolean;
  onOpen: () => void;
  onWake?: () => void;
  waking?: boolean;
}

export function ComputerPreview({ bot, transport, sandboxState, inFullWindow, onOpen, onWake, waking }: ComputerPreviewProps) {
  const live = (sandboxState === null || sandboxState === 'running') && !inFullWindow;
  const view = useScreenLink({ transport, botId: bot.id, mode: 'view', active: live });

  if (!live) {
    const starting = waking || sandboxState === 'starting';
    return (
      <div className="ck-potato-computer-view ck-potato-computer-view--idle">
        <div className="ck-potato-computer-note" role="status">
          {inFullWindow ? (
            <span>{COMPUTER_COPY.inFullWindow}</span>
          ) : (
            <>
              <span className="ck-potato-computer-asleep">{starting ? COMPUTER_COPY.waking : sandboxState === 'failed' ? COMPUTER_COPY.couldNotWake : COMPUTER_COPY.asleep}</span>
              {onWake && !starting && (
                <button type="button" className="ck-btn ck-btn--secondary" onClick={onWake}>
                  {sandboxState === 'failed' ? 'Start again' : COMPUTER_COPY.wake}
                </button>
              )}
            </>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="ck-potato-computer-view">
      {view.link && (
        <iframe
          key={`${view.link.mode}-${view.attempt}`}
          ref={view.frame}
          className="ck-potato-computer-frame"
          title={`${bot.name}'s computer`}
          src={view.link.src}
          sandbox="allow-scripts allow-same-origin"
          allow="clipboard-read; clipboard-write"
          tabIndex={-1}
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
      {view.title && <p className="ck-potato-computer-window">{view.title}</p>}
      <button type="button" className="ck-potato-computer-open" aria-label={COMPUTER_COPY.openLabel} title={COMPUTER_COPY.openLabel} onClick={onOpen}>
        <span className="ck-potato-computer-open-pill">{COMPUTER_COPY.open}</span>
      </button>
    </div>
  );
}
