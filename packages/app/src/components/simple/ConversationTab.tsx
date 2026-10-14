// Conversation: what she asked and what the agent did, in plain words. Drawn from the run
// transcript (the same data as the Transcript tab) and the last turn's numbers.
import { DotSpinner } from '../cockpit/DotSpinner';
import type { PaneTranscript, TurnSummary } from '../../lib/cockpit-api';
import { conversationItems, summarySentence } from './conversation-model';
import { T } from './copy';

export function ConversationTab({
  transcript,
  error,
  lastTurn,
  agent,
  working,
}: {
  /** `undefined` = still loading, `null` = this pane has no transcript. */
  transcript: PaneTranscript | null | undefined;
  error: string | null;
  lastTurn: TurnSummary | null | undefined;
  agent: string;
  working: boolean;
}) {
  if (error) return <p className="sx-quiet sx-pad">{T.conversationUnavailable}</p>;
  const items = conversationItems(transcript);
  const summary = working ? null : summarySentence(lastTurn, agent);
  if (items.length === 0 && !working) {
    return <p className="sx-quiet sx-pad">{transcript === undefined ? T.thinking : T.conversationEmpty}</p>;
  }
  return (
    <div className="sx-convo" role="log" aria-label={T.tabConversation}>
      {items.map((item, i) => {
        if (item.kind === 'you') {
          return (
            <div key={i} className="sx-msg sx-msg--you">
              <span className="sx-who">{T.you}</span>
              <p>{item.text}</p>
            </div>
          );
        }
        if (item.kind === 'agent') {
          return (
            <div key={i} className="sx-msg">
              <span className="sx-who">{agent}</span>
              <p>{item.text}</p>
            </div>
          );
        }
        return (
          <p key={i} className={`sx-step${item.kind === 'problem' ? ' sx-step--problem' : ''}`}>
            {item.text}
          </p>
        );
      })}
      {working && (
        <p className="sx-step sx-step--live">
          <span aria-hidden="true">
            <DotSpinner status="running" />
          </span>
          {T.thinking}
        </p>
      )}
      {summary && (
        <div className="sx-summary">
          <span className="sx-who">{T.summary}</span>
          <p>{summary}</p>
        </div>
      )}
    </div>
  );
}
