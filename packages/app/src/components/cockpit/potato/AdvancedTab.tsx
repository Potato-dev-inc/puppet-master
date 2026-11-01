// The Advanced tab of the bot panel (per-bot settings). The settings phases replace this file; its props stay.
// Read replies aloud is the first section (voice phase P12); the rest of the tab is still coming.
import type { PotatoSettings, PotatoTransport } from '../../../lib/potato';
import { PANEL_COPY } from '../../../lib/potato-panel';
import { ComingSoon } from './ComingSoon';
import { ReadAloudSection } from './ReadAloudSection';

export interface AdvancedTabProps {
  bot: { id: string; name: string };
  settings?: PotatoSettings;
  transport?: PotatoTransport;
  onChanged?: () => void;
}

export function AdvancedTab({ bot, settings, transport, onChanged }: AdvancedTabProps) {
  return (
    <>
      <ReadAloudSection bot={bot} settings={settings} transport={transport} onChanged={onChanged} />
      <ComingSoon line={PANEL_COPY.advanced} />
    </>
  );
}
