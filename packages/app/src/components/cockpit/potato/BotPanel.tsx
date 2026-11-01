// The right bot panel (spec: Rakazo's side panel): who the bot is, a close button, and three tabs. Computer holds the
// bot's sandbox screen; Routines and Advanced are placeholders until their phases. Every tab stays mounted while the
// panel is open, so the screen does not reload and a draft in Advanced survives a tab switch.
import type { AvatarId } from '../../../lib/companion';
import { PANEL_COPY, PANEL_TABS, type PanelTab } from '../../../lib/potato-panel';
import type { SandboxTransport } from '../../../lib/potato-sandbox';
import type { BrainKind } from '../../../lib/potato';
import type { BotSettingsTransport } from '../../../lib/potato-botsettings';
import type { ScreenTransport } from '../../../lib/potatoScreen';
import { Avatar } from '../../companion/Avatar';
import { SegTabs } from '../SegTabs';
import { AdvancedTab } from './AdvancedTab';
import { ComputerTab } from './ComputerTab';
import { RoutinesTab } from './RoutinesTab';
import { Glyph } from './ui/Glyph';
import '../../../styles/cockpit-potato-panel.css';

export interface BotPanelProps {
  bot: { id: string; name: string };
  avatar: AvatarId;
  tab: PanelTab;
  onTab: (tab: PanelTab) => void;
  onClose: () => void;
  screen?: ScreenTransport;
  /** The sandbox's commands (start, stop, set up). Absent: the Computer tab has no upkeep section. */
  sandbox?: SandboxTransport;
  /** Opens Settings > Potato Bot (Sandbox). */
  onOpenSettings?: () => void;
  /** "Through my agents": the bot has no computer of its own. */
  through: boolean;
  onNotify: (text: string) => void;
  /** The Settings brain: what a bot without its own brain uses (the Advanced tab's Model row). */
  defaultBrain?: BrainKind;
  /** After a rename or a delete: the page reads its state again. */
  onChanged?: () => void;
  /** Tests pass a fake; the app passes nothing. */
  botSettings?: BotSettingsTransport;
}

export function BotPanel({ bot, avatar, tab, onTab, onClose, screen, sandbox, onOpenSettings, through, onNotify, defaultBrain, onChanged, botSettings }: BotPanelProps) {
  return (
    <div className="ck-potato-botpanel">
      <header className="ck-potato-panel-head">
        <Avatar id={avatar} size={24} state="idle" name={bot.name} />
        <h2 className="ck-potato-panel-title">{bot.name}</h2>
        <button type="button" className="ck-icon-btn" aria-label={PANEL_COPY.close} title={PANEL_COPY.close} onClick={onClose}>
          <Glyph name="close" />
        </button>
      </header>
      <div className="ck-potato-panel-tabs">
        <SegTabs options={PANEL_TABS.map((t) => ({ ...t }))} value={tab} onChange={onTab} label="Bot panel" small />
      </div>
      <div className="ck-scroll ck-potato-panel-body">
        <div className="ck-potato-tabpanel" role="tabpanel" aria-label="Computer" hidden={tab !== 'computer'}>
          <ComputerTab bot={bot} avatar={avatar} screen={screen} sandbox={sandbox} through={through} onNotify={onNotify} onOpenSettings={onOpenSettings} />
        </div>
        <div className="ck-potato-tabpanel" role="tabpanel" aria-label="Routines" hidden={tab !== 'routines'}>
          <RoutinesTab bot={bot} />
        </div>
        <div className="ck-potato-tabpanel" role="tabpanel" aria-label="Advanced" hidden={tab !== 'advanced'}>
          <AdvancedTab bot={bot} defaultBrain={defaultBrain} transport={botSettings} onChanged={onChanged} onNotify={onNotify} />
        </div>
      </div>
    </div>
  );
}
