// The cards Simple mode docks above the tabs: a question with Yes and No, a stop PM made by itself,
// a problem with one button. Short, one idea each, and never the word for a failure.
import { useState } from 'react';
import type { PanePrompt, PromptChoice } from '../../lib/cockpit-api';
import { gestureOf, type UserGesture } from '../../lib/user-gesture';
import { DotSpinner } from '../cockpit/DotSpinner';
import { simpleApproval, yesNo } from './approval-text';
import { F, T } from './copy';
import type { Problem } from './error-text';

export function AskCard({ agent, prompt, onAnswer }: { agent: string; prompt: PanePrompt; onAnswer: (choice: PromptChoice, gesture: UserGesture) => void }) {
  const [details, setDetails] = useState(false);
  const words = simpleApproval(prompt);
  if (!words || words.kind !== 'ask') return null;
  const { yes, no } = yesNo(prompt.choices);
  const command = (prompt.summary || prompt.text || '').trim();
  const careful = words.level === 'medium';
  return (
    <section className="sx-card" role="group" aria-label={F.agentWants(agent, words.what, words.safe)} data-testid="ask-card">
      <span className="sx-card-glyph" aria-hidden="true">
        <DotSpinner status="waiting_input" />
      </span>
      <div className="sx-card-body">
        <p className="sx-card-text">{F.agentWants(agent, words.what, words.safe)}</p>
        {careful && <p className="sx-card-tag">{T.checkFirst}</p>}
        {command && (
          <button type="button" className="sx-link" aria-expanded={details} onClick={() => setDetails((d) => !d)}>
            {details ? T.hideChange : T.willChange}
          </button>
        )}
        {details && <pre className="sx-card-cmd">{command}</pre>}
      </div>
      <div className="sx-card-actions">
        {no && (
          <button type="button" className="ck-btn ck-btn--secondary" onClick={(event) => onAnswer(no, gestureOf(event))}>
            {T.no}
          </button>
        )}
        {yes && (
          <button type="button" className={`ck-btn ${careful ? 'ck-btn--secondary' : 'ck-btn--primary'}`} onClick={(event) => onAnswer(yes, gestureOf(event))}>
            {T.yes}
          </button>
        )}
      </div>
    </section>
  );
}

/** PM answered No by itself. Nothing waits on her: this only tells her. */
export function BlockedCard({
  agent,
  what,
  onOk,
  onTell,
}: {
  agent: string;
  what: string;
  onOk: () => void;
  onTell: () => void;
}) {
  const [why, setWhy] = useState(false);
  return (
    <section className="sx-card sx-card--blocked" role="group" aria-label={F.stoppedAgent(agent)} data-testid="blocked-card">
      <span className="sx-card-glyph" aria-hidden="true">
        <DotSpinner status="error" />
      </span>
      <div className="sx-card-body">
        <p className="sx-card-text">{F.stoppedAgent(agent)}</p>
        <p className="sx-card-text">{F.blockedWanted(agent, what)}</p>
        <p className="sx-card-text">{T.blockedSafe}</p>
        <button type="button" className="sx-link" aria-expanded={why} onClick={() => setWhy((w) => !w)}>
          {T.blockedWhy}
        </button>
        {why && <p className="sx-quiet">{T.blockedWhyBody}</p>}
      </div>
      <div className="sx-card-actions">
        <button type="button" className="ck-btn ck-btn--secondary" onClick={onTell}>
          {T.blockedTell}
        </button>
        <button type="button" className="ck-btn ck-btn--primary" autoFocus onClick={onOk}>
          {T.blockedOk}
        </button>
      </div>
    </section>
  );
}

export function ProblemCard({ problem, onStartAgain, onShowMe }: { problem: Problem; onStartAgain: () => void; onShowMe: () => void }) {
  return (
    <section className="sx-card" role="group" aria-label={problem.text} data-testid="problem-card">
      <span className="sx-card-glyph" aria-hidden="true">
        <DotSpinner status="error" />
      </span>
      <div className="sx-card-body">
        <p className="sx-card-text">{problem.text}</p>
      </div>
      <div className="sx-card-actions">
        {problem.button === 'start_again' && (
          <button type="button" className="ck-btn ck-btn--primary" onClick={onStartAgain}>
            {T.tryAgain}
          </button>
        )}
        {problem.button === 'show_me' && (
          <button type="button" className="ck-btn ck-btn--secondary" onClick={onShowMe}>
            {T.showMe}
          </button>
        )}
      </div>
    </section>
  );
}
