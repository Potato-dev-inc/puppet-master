// The Computer tab of the bot panel: the Computer panel (screen, who has it, Take over / Hand back / Skip), or a short
// line that says why there is none (Sandbox is off, or the bot talks through the person's agents).
import { PANEL_COPY } from '../../../lib/potato-panel';
import type { ScreenTransport } from '../../../lib/potatoScreen';
import { ComputerPanel } from './ComputerPanel';

export interface ComputerTabProps {
  bot: { id: string; name: string };
  screen?: ScreenTransport;
  through: boolean;
  onNotify: (text: string) => void;
}

export function ComputerTab({ bot, screen, through, onNotify }: ComputerTabProps) {
  if (through) return <p className="ck-sub ck-potato-panel-empty">{PANEL_COPY.noComputerThrough}</p>;
  const off = <p className="ck-sub ck-potato-panel-empty">{PANEL_COPY.noComputer(bot.name)}</p>;
  if (!screen) return off;
  return <ComputerPanel bot={bot} transport={screen} onNotify={onNotify} whenOff={off} />;
}
