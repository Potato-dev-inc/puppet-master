// The "Needs you" banner in a bot's thread. A bot that is stuck on the computer asks the person; the Computer tab says so,
// but while the bot panel is closed (or on another tab) that would be easy to miss, so the thread says it too, with a
// button that opens the Computer tab. It is there only while this bot is the one asking and the tab is not showing.
import { PANEL_COPY } from '../../../lib/potato-panel';
import type { ScreenState } from '../../../lib/potatoScreen';
import { StatusPill } from './ui/StatusPill';
import '../../../styles/cockpit-potato-thread.css';

/** Whether the banner is due: this bot asks, and the Computer tab is not on screen. */
export function bannerDue(screen: ScreenState | null, botId: string, computerShown: boolean): boolean {
  return !!screen && screen.enabled && screen.phase === 'needs_you' && screen.bot === botId && !computerShown;
}

export interface ComputerBannerProps {
  botName: string;
  reason: string | null;
  onOpen: () => void;
}

export function ComputerBanner({ botName, reason, onOpen }: ComputerBannerProps) {
  return (
    <div className="ck-potato-banner" role="status">
      <StatusPill tone="strong">Needs you</StatusPill>
      <p className="ck-potato-banner-text">
        {PANEL_COPY.needsYou(botName)}
        {reason && <span className="ck-sub ck-potato-banner-reason"> {reason}</span>}
      </p>
      <button type="button" className="ck-btn ck-btn--secondary" onClick={onOpen}>
        {PANEL_COPY.openComputer}
      </button>
    </div>
  );
}
