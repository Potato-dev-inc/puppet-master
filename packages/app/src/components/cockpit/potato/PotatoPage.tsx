// The Potato Bot page in the cockpit's main area (spec section 4): three regions. The bot list on the left, the open
// thread in the middle and, when the person opens it, the bot panel on the right (Computer, Routines, Advanced).
// It is reachable only while the Labs switch is on (the caller renders it only then). A bot is a companion, so
// "New bot" opens the companion sheet and turns what it makes into a bot.
import { useCallback, useMemo, useState } from 'react';
import type { AvatarId, CompanionTransport, CompanionView } from '../../../lib/companion';
import { plainError, type PotatoState, type PotatoTransport } from '../../../lib/potato';
import type { SandboxTransport } from '../../../lib/potato-sandbox';
import type { ScreenTransport } from '../../../lib/potatoScreen';
import { usePotatoThread } from '../../../hooks/usePotato';
import type { BotSettingsTransport } from '../../../lib/potato-botsettings';
import { CompanionSetupSheet } from '../companion/CompanionSetupSheet';
import { DEFAULT_TAB, type PanelTab } from '../../../lib/potato-panel';
import { BotList } from './BotList';
import { BotPanel } from './BotPanel';
import { PotatoLayout } from './PotatoLayout';
import { ThreadView } from './ThreadView';
import { pickOpenBot } from './model';
import { usePotatoKeys } from './potato-keys';
import { usePotatoLayout } from './usePotatoLayout';

export interface PotatoPageProps {
  state: PotatoState;
  /** Goes up on every change; the open thread reads again when it moves. */
  version: number;
  transport: PotatoTransport;
  companionTransport: CompanionTransport;
  /** The companions: a bot's face and agent come from its companion. Null: no companion backend. */
  companions: CompanionView[] | null;
  openBotId: string | null;
  onOpenBot: (botId: string) => void;
  /** Called after a command, so the state is read again at once. */
  onChanged: () => void;
  /** Called after the sheet made a companion, so the companion list is read again. */
  onCompanionsChanged: () => void;
  onNotify: (text: string) => void;
  onOpenSettings: () => void;
  /** Opens a companion's own page ("Through my agents"). */
  onOpenCompanion?: (id: string) => void;
  /** Opens a helper's pane (the Open button of a finished helper), and says whether the pane is still there. */
  onOpenPane?: (paneId: string) => void;
  paneExists?: (paneId: string) => boolean;
  /** The Advanced tab's calls to the engine. Absent: the app's own. */
  botSettings?: BotSettingsTransport;
  /** The Computer panel's way to the app (the sandbox screen). Absent: no panel. */
  screenTransport?: ScreenTransport;
  /** The sandbox's commands for the Computer tab's upkeep section and its set up card. Absent: neither is drawn. */
  sandboxTransport?: SandboxTransport;
}

export function PotatoPage(props: PotatoPageProps) {
  const { state, version, transport, companionTransport, companions, openBotId, onOpenBot, onChanged, onCompanionsChanged, onNotify, onOpenSettings, onOpenCompanion, onOpenPane, paneExists, screenTransport, sandboxTransport, botSettings } = props;
  const [sheetOpen, setSheetOpen] = useState(false);
  const open = pickOpenBot(state.bots, openBotId);
  const layout = usePotatoLayout();
  // The tab each bot's panel was last on (not remembered between runs).
  const [tabs, setTabs] = useState<Record<string, PanelTab>>({});
  const tab: PanelTab = (open && tabs[open.id]) || DEFAULT_TAB;
  const setTab = useCallback((id: string, next: PanelTab) => setTabs((was) => ({ ...was, [id]: next })), []);
  usePotatoKeys({ toggleList: layout.toggleList, togglePanel: layout.togglePanel });
  const { messages } = usePotatoThread(transport, open?.id ?? null, version, open?.unread ?? 0);

  const byId = useMemo(() => new Map((companions ?? []).map((c) => [c.id, c])), [companions]);
  const avatars = useMemo(() => new Map<string, AvatarId>([...byId].map(([id, c]) => [id, c.avatar])), [byId]);
  const candidates = useMemo(() => (companions ?? []).filter((c) => !state.bots.some((b) => b.id === c.id)), [companions, state.bots]);

  const makeBot = async (companionId: string) => {
    try {
      await transport.addBot(companionId);
      onChanged();
      onOpenBot(companionId);
    } catch (error) {
      onNotify(plainError(error, 'Could not make a bot.'));
    }
  };

  // A bot's own brain decides (the engine reports `agents` for it, default or not), not the Settings brain alone.
  const through = !!open && open.brainState === 'agents';
  const panelOpen = layout.panelOpen && !!open;

  // The row menu's commands: each one runs, then the state is read again at once.
  const botCommand = (what: string, run: (botId: string) => Promise<void>) => (botId: string) => {
    run(botId)
      .catch((error) => onNotify(plainError(error, `Could not ${what}.`)))
      .finally(onChanged);
  };
  const botActions = {
    pause: botCommand('pause the bot', (id) => transport.stopBot(id)),
    resume: botCommand('start the bot', (id) => transport.startBot(id)),
    markRead: botCommand('mark the bot read', (id) => transport.markRead(id)),
  };

  const list = (
    <BotList
      bots={state.bots}
      avatars={avatars}
      openId={open?.id ?? null}
      onOpen={onOpenBot}
      onNew={() => setSheetOpen(true)}
      candidates={candidates}
      onUseCompanion={(id) => void makeBot(id)}
      mini={layout.listMini}
      onToggleMini={layout.toggleList}
      actions={botActions}
    />
  );
  const thread = open ? (
    <ThreadView
      key={open.id}
      bot={open}
      messages={messages}
      settings={state.settings}
      avatar={avatars.get(open.id) ?? 'mochi'}
      agentLabel={byId.get(open.id)?.agentLabel ?? 'its agent'}
      transport={transport}
      onOpenSettings={onOpenSettings}
      onNotify={onNotify}
      onChanged={onChanged}
      companionPaneId={byId.get(open.id)?.paneId ?? null}
      onOpenCompanion={onOpenCompanion}
      onOpenPane={onOpenPane}
      paneExists={paneExists}
      screen={screenTransport}
      onNewBot={() => setSheetOpen(true)}
      panel={{
        open: panelOpen,
        computerShown: panelOpen && tab === 'computer',
        onToggle: layout.togglePanel,
        onOpenComputer: () => {
          setTab(open.id, 'computer');
          layout.setPanelOpen(true);
        },
      }}
    />
  ) : (
    <div className="ck-potato-blank" aria-hidden="true" />
  );
  const panel = panelOpen && open ? (
    <BotPanel
      key={open.id}
      bot={open}
      avatar={avatars.get(open.id) ?? 'mochi'}
      tab={tab}
      onTab={(next) => setTab(open.id, next)}
      onClose={() => layout.setPanelOpen(false)}
      screen={screenTransport}
      sandbox={sandboxTransport}
      onOpenSettings={onOpenSettings}
      through={through}
      onNotify={onNotify}
      defaultBrain={state.settings.brain}
      botSettings={botSettings}
      onChanged={onChanged}
    />
  ) : null;

  return (
    <>
      <PotatoLayout layout={layout} list={list} thread={thread} panel={panel} />
      {sheetOpen && (
        <CompanionSetupSheet
          transport={companionTransport}
          onClose={() => setSheetOpen(false)}
          onCreated={(created) => {
            setSheetOpen(false);
            onCompanionsChanged();
            void makeBot(created.id);
          }}
        />
      )}
    </>
  );
}
