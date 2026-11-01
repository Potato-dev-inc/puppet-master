// The Advanced tab of the bot panel (per-bot settings). The settings phases replace this file; its props stay.
import { PANEL_COPY } from '../../../lib/potato-panel';
import { ComingSoon } from './ComingSoon';

export function AdvancedTab(_props: { bot: { id: string; name: string } }) {
  return <ComingSoon line={PANEL_COPY.advanced} />;
}
