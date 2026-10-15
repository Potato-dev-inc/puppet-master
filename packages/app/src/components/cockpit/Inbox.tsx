import { useEffect, useRef, type ReactNode } from 'react';
import type { CockpitApi } from '../../hooks/useCockpit';
import { projectWithCopy } from './shell-labels';
import { hasEverSpawned } from '../../lib/agent-health';
import { sortInbox, type InboxEntry } from '../../lib/inbox-order';
import { isBudget, isConsent, isLimit, type QuestionAnswer } from '../../lib/questions';
import { patchSettings } from '../../lib/settings';
import { ApprovalPrompt } from './ApprovalPrompt';
import { PaneStuckCard } from './StuckCard';
import { blockingItems, itemsOfKind } from '../../lib/attention';
import { useNeedsYou } from '../../hooks/useNeedsYou';
import { AskNotices } from './AskNotices';
import { CanaryNotices } from './CanaryNotices';
import { promptSummary } from './model';
import { BudgetCard } from './BudgetCard';
import { ConsentCard } from './ConsentCard';
import { LimitCard } from './LimitCard';
import { QuestionCard } from './QuestionCard';
import { PaneErrorCard } from './InboxError';
import { CardButton, InboxCard, focusWithRing } from './InboxCard';
import { handleInboxKey, isEditable } from './inbox-keys';
import { paneHeading } from './titles';
import { TeamSection } from './team/TeamSection';

/** One card in the Inbox's single list: where it sorts, and how to draw it. */
interface Entry extends InboxEntry {
  /** A number key can answer it (not high risk, not consent). Drives the keyboard hint. */
  quick: boolean;
  render: (first: boolean) => ReactNode;
}

export function Inbox({ c, onOpenDoctor }: { c: CockpitApi; onOpenDoctor?: () => void }) {
  // One list for every surface: the badge, the Doto counter and the sidebar group use its length.
  const needs = useNeedsYou(c);
  const count = blockingItems(needs).length;
  const waiting = c.registry.paneList.filter((p) => itemsOfKind(needs, 'approval').some((i) => i.paneId === p.info.id));
  const errored = c.registry.paneList.filter((p) => itemsOfKind(needs, 'error').some((i) => i.paneId === p.info.id));
  const noPanes = c.registry.paneList.length === 0 && c.registry.initialReady;
  const firstRun = noPanes && !hasEverSpawned();
  const stuckIds = itemsOfKind(needs, 'stuck').flatMap((i) => (i.paneId ? [i.paneId] : []));

  const entries = (() => {
    const out: Entry[] = [];
    const answer = (id: string) => (a: QuestionAnswer) => void c.asked.answerQuestion(id, a);
    const open = (paneId?: string) => (paneId && c.registry.panes.has(paneId) ? () => c.select(paneId) : undefined);
    const age = (since?: number) => (since === undefined ? null : c.now - since);

    for (const q of c.asked.questions) {
      const paneName = (q.consent?.pane_id ?? q.pane_id) ? c.names.get((q.consent?.pane_id ?? q.pane_id)!) : undefined;
      if (isConsent(q)) {
        out.push({
          key: `consent:${q.id}`,
          kind: 'consent',
          since: q.created_at_ms,
          quick: false,
          render: (first) => (
            <ConsentCard q={q} paneName={paneName} now={c.now} autoFocus={first} onAnswer={answer(q.id)} onOpenPane={open(q.consent?.pane_id)} />
          ),
        });
      } else if (isBudget(q)) {
        out.push({
          key: `budget:${q.id}`,
          kind: 'budget',
          since: q.created_at_ms,
          quick: true,
          render: () => (
            <BudgetCard q={q} paneName={paneName} now={c.now} onAnswer={answer(q.id)} onDismiss={() => void c.asked.dismissQuestion(q.id)} onOpenPane={open(q.pane_id)} />
          ),
        });
      } else if (isLimit(q)) {
        out.push({
          key: `limit:${q.id}`,
          kind: 'limit',
          since: q.created_at_ms,
          quick: true,
          render: () => <LimitCard q={q} paneName={paneName} now={c.now} onAnswer={answer(q.id)} onOpenPane={open(q.pane_id)} />,
        });
      } else {
        out.push({
          key: `question:${q.id}`,
          kind: 'question',
          since: q.created_at_ms,
          quick: q.choices.length > 0,
          render: () => (
            <QuestionCard q={q} paneName={paneName} now={c.now} onAnswer={answer(q.id)} onDismiss={() => void c.asked.dismissQuestion(q.id)} onOpenPane={open(q.pane_id)} />
          ),
        });
      }
    }

    for (const p of waiting) {
      const id = p.info.id;
      const prompt = c.prompts[id];
      const since = c.statusSince.get(id);
      const heading = paneHeading(p.info, c.names.get(id) ?? p.info.agent_type);
      const who = [heading.primary, heading.secondary, projectWithCopy(p.info.cwd)].filter(Boolean).join(' · ');
      const risk = prompt?.risk?.level;
      const hasChoices = !!prompt && prompt.choices.length > 0;
      out.push({
        key: `approval:${id}`,
        kind: 'approval',
        risk,
        since,
        quick: hasChoices && risk !== 'high',
        render: (first) =>
          prompt?.risk && hasChoices ? (
            <ApprovalPrompt
              variant="card"
              prompt={prompt}
              who={who}
              age={age(since)}
              autoFocus={first}
              onOpen={() => c.select(id)}
              onAnswer={(ch) => void c.answer(id, prompt.promptId, ch.id, ch.label)}
            />
          ) : (
            // A menu the agent drew that Puppet Master could not rate: its own words, equal weight, no default.
            <InboxCard
              kind="approval"
              glyph="needs"
              title={prompt ? promptSummary(prompt) || 'Waiting for your answer' : 'Waiting for your answer'}
              meta={who}
              age={age(since)}
              actions={
                hasChoices ? (
                  <>
                    {prompt.choices.map((ch) => (
                      <CardButton key={ch.id} tone="secondary" role="answer" onClick={() => void c.answer(id, prompt.promptId, ch.id, ch.label)}>
                        {ch.label}
                      </CardButton>
                    ))}
                  </>
                ) : null
              }
              more={
                <CardButton tone="plain" role="open" onClick={() => c.select(id)}>
                  Open terminal
                </CardButton>
              }
            />
          ),
      });
    }

    for (const p of errored) {
      const id = p.info.id;
      const since = c.statusSince.get(id);
      out.push({
        key: `error:${id}`,
        kind: 'error',
        since,
        quick: true,
        render: () => (
          <PaneErrorCard
            c={c}
            paneId={id}
            agentType={p.info.agent_type}
            name={c.names.get(id) ?? p.info.agent_type}
            project={projectWithCopy(p.info.cwd)}
            age={age(since)}
            onOpenDoctor={onOpenDoctor}
          />
        ),
      });
    }

    for (const id of stuckIds) {
      out.push({ key: `stuck:${id}`, kind: 'stuck', quick: true, render: () => <PaneStuckCard c={c} paneId={id} dock={false} /> });
    }
    return sortInbox(out);
  })();

  // The keyboard works without the mouse: when the Inbox first has a card, focus the first one
  // (a high-risk card has already put focus on its Deny). J / K move, 1 2 3 answer, O opens, Esc dismisses.
  const root = useRef<HTMLDivElement>(null);
  const focused = useRef(false);
  useEffect(() => {
    if (focused.current || entries.length === 0) return;
    focused.current = true;
    const box = root.current;
    const active = document.activeElement;
    if (!box || (active && box.contains(active))) return;
    if (active && active !== document.body && isEditable(active)) return;
    if (document.querySelector('.ck-sheet, [aria-modal="true"], .ck-away')) return;
    focusWithRing(box.querySelector<HTMLElement>('[data-ib-card]'));
  }, [entries.length]);

  const quick = entries.some((e) => e.quick);
  const title = firstRun ? 'Welcome to Puppet Master' : count === 0 ? 'Nothing needs you' : count === 1 ? 'One thing needs you' : `${count} things need you`;

  return (
    <div
      ref={root}
      className="ck-inbox"
      tabIndex={-1}
      onKeyDown={(e) => {
        if (root.current) handleInboxKey(e.nativeEvent, root.current);
      }}
    >
      <header className="ck-inbox-hero">
        {!firstRun && (
          <div className="ck-doto" aria-label={`${count} ${count === 1 ? 'thing needs' : 'things need'} you`}>
            {String(count).padStart(2, '0')}
          </div>
        )}
        <h1 className="ck-title">{title}</h1>
        <p className="ck-sub">
          {firstRun
            ? 'Your AI helpers work here. First, check which ones this PC has.'
            : count === 0
            ? 'Agents that stop to ask a question will show up here.'
            : 'Answer here, or open the terminal.'}
        </p>
        {entries.length > 0 && (
          <p className="ib-hint" aria-label="Keyboard shortcuts">
            <span>
              <kbd className="ib-kbd">J</kbd>
              <kbd className="ib-kbd">K</kbd> move
            </span>
            {quick && (
              <span>
                <kbd className="ib-kbd">1</kbd>
                <kbd className="ib-kbd">2</kbd> answer
              </span>
            )}
            <span>
              <kbd className="ib-kbd">O</kbd> open
            </span>
          </p>
        )}
        {noPanes && onOpenDoctor && (
          <button type="button" className={`ck-btn ${firstRun ? 'ck-btn--primary' : 'ck-btn--secondary'}`} onClick={onOpenDoctor}>
            Set up your agents
          </button>
        )}
        {firstRun && (
          <p className="ib-welcome-alt">
            <span>I don't write code.</span>
            <button type="button" className="ck-btn ck-btn--plain" onClick={() => void patchSettings({ simple_mode: true }).catch(() => undefined)}>
              Use Simple mode
            </button>
          </p>
        )}
      </header>

      {entries.length > 0 && (
        <ul className="ib-list" aria-label="Needs you">
          {entries.map((e, i) => (
            <li key={e.key}>{e.render(i === 0)}</li>
          ))}
        </ul>
      )}

      <AskNotices />
      <CanaryNotices />

      <TeamSection now={c.now} paneNames={c.names} />
    </div>
  );
}
