// The Potato Bot page in the cockpit's main area (spec section 4): a 240 px list of bots next to the open thread.
// It is reachable only while the Labs switch is on (the caller renders it only then). A bot is a companion, so
// "New bot" opens the companion sheet and turns what it makes into a bot.
import { useMemo, useState } from 'react';
import type { AvatarId, CompanionTransport, CompanionView } from '../../../lib/companion';
import { plainError, type PotatoState, type PotatoTransport } from '../../../lib/potato';
import { usePotatoThread } from '../../../hooks/usePotato';
import { CompanionSetupSheet } from '../companion/CompanionSetupSheet';
import { ConversationList } from './ConversationList';
import { ThreadView } from './ThreadView';
import { pickOpenBot } from './model';
import '../../../styles/cockpit-potato.css';

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
}

export function PotatoPage(props: PotatoPageProps) {
  const { state, version, transport, companionTransport, companions, openBotId, onOpenBot, onChanged, onCompanionsChanged, onNotify, onOpenSettings, onOpenCompanion } = props;
  const [sheetOpen, setSheetOpen] = useState(false);
  const open = pickOpenBot(state.bots, openBotId);
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

  return (
    <div className="ck-potato-page">
      <ConversationList
        bots={state.bots}
        avatars={avatars}
        openId={open?.id ?? null}
        onOpen={onOpenBot}
        onNew={() => setSheetOpen(true)}
        candidates={candidates}
        onUseCompanion={(id) => void makeBot(id)}
      />
      <div className="ck-potato-main">
        {open ? (
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
            onOpenCompanion={onOpenCompanion}
          />
        ) : (
          <div className="ck-potato-blank" aria-hidden="true" />
        )}
      </div>
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
    </div>
  );
}
