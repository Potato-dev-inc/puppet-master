// The read-only view of one old chat ("Bring my old chats"), opened from a search hit. Only the search text is
// stored, so the chat is read again from the agent's own file when it opens. It shows in the normal chat view with
// no message box, and a banner says it is old and read-only. "Continue in a pane" starts the agent on this chat with
// the agent's own resume option, when the agent has one. The plan comes from the backend; nothing is typed.
import { useEffect, useId, useMemo, useState } from 'react';
import {
  chatImportApi,
  importedChatKey,
  type ImportApi,
  type ImportedChatRequest,
  type OpenedImportedChat,
  type ResumePlan,
} from '../../lib/chatImport';
import { agentLabel, useAgentRegistry } from '../../lib/agentRegistry';
import type { ChatJump } from '../../lib/chatJump';
import { groupTurns } from '../../lib/sessionChatModel';
import { SessionChatView } from '../chat/SessionChatView';
import { Sheet } from '../cockpit/Sheet';
import '../../styles/history-viewer.css';

/** "14 Aug", with the year added when the chat is from another year. */
export function chatDate(ms: number, now: number = Date.now()): string {
  const at = new Date(ms);
  const day = new Intl.DateTimeFormat('en-GB', { day: 'numeric', month: 'short' }).format(at);
  return at.getFullYear() === new Date(now).getFullYear() ? day : `${day} ${at.getFullYear()}`;
}

/** The banner line: "Old Codex CLI chat from 14 Aug. Read-only." (`label` is the agent's display name, not its id). */
export function bannerText(label: string, updatedMs: number, now?: number): string {
  return `Old ${label} chat from ${chatDate(updatedMs, now)}. Read-only.`;
}

function errorText(error: unknown, fallback: string): string {
  const text = error instanceof Error ? error.message : typeof error === 'string' ? error : '';
  return text.trim() || fallback;
}

type Loaded = { kind: 'loading' } | { kind: 'unavailable'; reason: string } | { kind: 'ready'; chat: OpenedImportedChat };

export interface ImportedChatSheetProps {
  request: ImportedChatRequest;
  api?: Pick<ImportApi, 'open' | 'resumePlan'>;
  /** Called with the plan when the person picks "Continue in a pane" (only when the plan says ok). */
  onContinue: (plan: ResumePlan) => void;
  onClose: () => void;
}

export function ImportedChatSheet({ request, api = chatImportApi, onContinue, onClose }: ImportedChatSheetProps) {
  const { agent, session, itemId, nonce } = request;
  const titleId = useId();
  // The banner shows the agent's display name from the registry ("Claude Code"), never its id ("claude").
  const agentName = agentLabel(useAgentRegistry(), agent);
  const [loaded, setLoaded] = useState<Loaded>({ kind: 'loading' });
  const [plan, setPlan] = useState<ResumePlan | null>(null);

  // A new request (a new nonce) reads the chat again, even for the same agent and session.
  useEffect(() => {
    let live = true;
    setLoaded({ kind: 'loading' });
    setPlan(null);
    api.open(agent, session).then(
      (chat) => {
        if (live) setLoaded({ kind: 'ready', chat });
      },
      (error: unknown) => {
        if (live) setLoaded({ kind: 'unavailable', reason: errorText(error, 'This chat could not be opened.') });
      },
    );
    api.resumePlan(agent, session).then(
      (next) => {
        if (live) setPlan(next);
      },
      () => {
        if (live) setPlan({ ok: false, reason: "Could not check this chat's folder.", agentType: agent, cwd: '', extraArgs: [] });
      },
    );
    return () => {
      live = false;
    };
  }, [api, agent, session, nonce]);

  const grouped = useMemo(() => (loaded.kind === 'ready' ? groupTurns(loaded.chat.items) : null), [loaded]);

  // The jump uses a negative nonce: jumps from chatJump.ts count up from 1, so clearing this one never touches them.
  const jump = useMemo<ChatJump | null>(
    () => (itemId ? { paneId: importedChatKey(agent, session), itemId, nonce: -nonce } : null),
    [agent, session, itemId, nonce],
  );

  const ready = loaded.kind === 'ready';
  const canContinue = ready && plan?.ok === true;
  const continueReason = ready && plan && !plan.ok ? (plan.reason ?? 'This chat cannot be continued.') : null;
  const title = ready && loaded.chat.meta.title ? loaded.chat.meta.title : 'Old chat';

  const continueButton = (
    <button
      type="button"
      className="ck-btn ck-btn--secondary history-viewer-continue"
      disabled={!canContinue}
      title={continueReason ?? undefined}
      onClick={() => {
        if (plan?.ok) onContinue(plan);
      }}
    >
      Continue in a pane
    </button>
  );

  return (
    <Sheet labelledBy={titleId} role="dialog" onClose={onClose} className="history-viewer">
      <h2 id={titleId} className="ck-section">
        {title}
      </h2>
      {ready && (
        <>
          <p className="ck-sub history-viewer-banner" role="note">
            {bannerText(agentName, loaded.chat.meta.updatedMs)}
          </p>
          {continueReason && <p className="ck-sub history-viewer-reason">{continueReason}</p>}
          {loaded.chat.meta.cutShort && <p className="ck-sub">Only the first part of this chat is shown.</p>}
        </>
      )}
      <div className="history-viewer-body">
        {loaded.kind === 'loading' ? (
          <p className="ck-sub history-viewer-status" role="status">
            Loading the chat…
          </p>
        ) : (
          <SessionChatView
            state={ready ? 'ready' : 'unavailable'}
            reason={loaded.kind === 'unavailable' ? loaded.reason : null}
            turns={grouped?.turns ?? []}
            model={grouped?.model ?? null}
            cliVersion={grouped?.cliVersion ?? null}
            source="file"
            jump={jump}
            readOnly
            headerExtra={continueButton}
          />
        )}
      </div>
      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn ck-btn--secondary" onClick={onClose}>
          Close
        </button>
      </div>
    </Sheet>
  );
}
