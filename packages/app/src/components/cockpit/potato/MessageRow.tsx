// One row of a thread: a bubble, a small centred note, or a helper card. The bot's words are Markdown (no raw HTML; a
// web link opens in the system browser); yours stay as typed. Each bubble has a hover toolbar (Copy, Reply). Transcript
// only decides the order and the lines between rows.
import { formatWhen, type PotatoMessage, type PotatoTransport } from '../../../lib/potato';
import { emitQuote } from '../../../lib/potato-thread';
import { tauri } from '../../../lib/tauri';
import { ChatMarkdown } from '../../chat/ChatMarkdown';
import { HelperCard } from './HelperCard';
import { HoverActions } from './HoverActions';
import { SaveAsFile } from './SaveAsFile';
import type { ThreadRow } from './model';

/** The app's external-link helper; outside the desktop shell it does nothing. */
const openInBrowser = (url: string) => {
  void Promise.resolve(tauri.openExternalUrl(url)).catch(() => {});
};

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
  /** Opens a link from the bot's text. Default: the system browser. */
  openLink?: (url: string) => void;
}

export function MessageRow({ row, botId, botName, agentLabel, transport, onChanged, onOpenPane, paneExists, now, openLink = openInBrowser }: MessageRowProps) {
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
      <div className={`ck-potato-bubble ck-potato-bubble--${row.side}${row.side === 'bot' ? ' ck-potato-bubble--md' : ''}`} title={formatWhen(m.at, now)}>
        {row.side === 'bot' ? <ChatMarkdown text={m.text} onOpenLink={openLink} /> : m.text}
      </div>
      <HoverActions text={m.text} onReply={() => emitQuote(botId, m.text)}>
        <SaveAsFile botId={botId} messageId={m.id} files={transport.artifacts} />
      </HoverActions>
    </li>
  );
}
