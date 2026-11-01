// One row of a thread: a bubble, a small centred note, or a helper card. The message-list phase grows this file
// (markdown, hover actions, reply lines); Transcript only decides the order.
import { formatWhen, type PotatoMessage, type PotatoTransport, type PotatoVoice } from '../../../lib/potato';
import { HelperCard } from './HelperCard';
import { SpeakButton } from './SpeakButton';
import type { ThreadRow } from './model';

export interface MessageRowProps {
  row: ThreadRow;
  botId: string;
  botName: string;
  agentLabel: string;
  transport: PotatoTransport;
  onChanged: () => void;
  /** Opens a helper's pane (the Open button of a finished helper). Absent: no button. */
  onOpenPane?: (paneId: string) => void;
  /** Whether that pane is still there; the button is shown only then. Default: yes. */
  paneExists?: (paneId: string) => boolean;
  now: number;
  /** The voice settings: a bot reply's Speak button reads with them. */
  voice?: PotatoVoice;
}

export function MessageRow({ row, botId, botName, agentLabel, transport, onChanged, onOpenPane, paneExists, now, voice }: MessageRowProps) {
  const m: PotatoMessage = row.message;
  if (row.type === 'note') {
    const canOpen = m.kind === 'helper_result' && !!m.paneId && !!onOpenPane && (paneExists?.(m.paneId) ?? true);
    return (
      <li className="ck-potato-note" role="note">
        {m.text}
        {canOpen && (
          <>
            {' '}
            <button type="button" className="ck-potato-note-open" onClick={() => onOpenPane?.(m.paneId!)}>
              Open
            </button>
          </>
        )}
      </li>
    );
  }
  if (row.type === 'card') {
    return (
      <li className="ck-potato-card-row">
        <HelperCard
          name={botName}
          task={m.text}
          project={m.project}
          agentLabel={agentLabel}
          open={row.open}
          onStart={async (folder) => {
            await transport.helperStart(botId, m.requestId ?? '', folder);
            onChanged();
          }}
          onNotNow={async () => {
            await transport.helperDecline(botId, m.requestId ?? '');
            onChanged();
          }}
          loadFolders={() => transport.helperFolders(botId)}
        />
      </li>
    );
  }
  return (
    <li className={`ck-potato-line ck-potato-line--${row.side}${row.first ? ' is-first' : ''}`}>
      <div className={`ck-potato-bubble ck-potato-bubble--${row.side}`} title={formatWhen(m.at, now)}>
        {m.text}
      </div>
      {row.side === 'bot' && <SpeakButton id={m.id} text={m.text} voice={voice} />}
    </li>
  );
}
