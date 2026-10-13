import type { CockpitApi } from '../../hooks/useCockpit';
import { projectName } from '../../lib/mobile-agent-tabs';
import { hasEverSpawned } from '../../lib/agent-health';
import { DotSpinner } from './DotSpinner';
import { useLoopGuard } from '../../lib/loop-guard';
import { PaneStuckCard } from './StuckCard';
import { AskNotices } from './AskNotices';
import { formatElapsed, promptSummary, spinnerStatus } from './model';

export function Inbox({ c, onOpenDoctor }: { c: CockpitApi; onOpenDoctor?: () => void }) {
  const waiting = c.registry.paneList.filter((p) => p.status === 'waiting_input');
  const count = waiting.length;
  const noPanes = c.registry.paneList.length === 0 && c.registry.initialReady;
  const firstRun = noPanes && !hasEverSpawned();
  const stuckIds = Object.keys(useLoopGuard().signals).filter((id) => c.registry.panes.has(id));

  return (
    <div className="ck-inbox">
      <header className="ck-inbox-hero">
        <div className="ck-doto" aria-label={`${count} waiting`}>
          {String(count).padStart(2, '0')}
        </div>
        <h1 className="ck-title">{firstRun ? 'Welcome to Puppet Master' : count === 0 ? 'Nothing needs you' : count === 1 ? 'Agent waiting for you' : 'Agents waiting for you'}</h1>
        <p className="ck-sub">
          {firstRun
            ? 'Your AI agents live here. Start by checking which ones work on this computer.'
            : count === 0
            ? 'Agents that stop to ask a question will show up here.'
            : 'Answer here or open the terminal.'}
        </p>
        {noPanes && onOpenDoctor && (
          <button
            type="button"
            className={`ck-btn ${firstRun ? 'ck-btn--primary' : 'ck-btn--secondary'}`}
            onClick={onOpenDoctor}
          >
            Set up your agents
          </button>
        )}
      </header>

      {stuckIds.length > 0 && (
        <ul className="ck-stuck-list" aria-label="Agents that seem stuck">
          {stuckIds.map((id) => (
            <li key={id}>
              <PaneStuckCard c={c} paneId={id} />
            </li>
          ))}
        </ul>
      )}

      <AskNotices />

      {count > 0 && (
        <ul className="ck-list">
          {waiting.map((p) => {
            const id = p.info.id;
            const prompt = c.prompts[id];
            const since = c.statusSince.get(id);
            return (
              <li key={id} className="ck-inbox-item">
                <div className="ck-inbox-top">
                  <span className="ck-row-glyph">
                    <DotSpinner status={spinnerStatus(p.status)} />
                  </span>
                  <span className="ck-row-title">{c.names.get(id) ?? p.info.agent_type}</span>
                  <span className="ck-row-sub">{projectName(p.info.cwd)}</span>
                  <span className="ck-row-time">{since ? `waiting ${formatElapsed(c.now - since)}` : ''}</span>
                </div>
                {prompt && <p className="ck-inbox-q">{promptSummary(prompt)}</p>}
                <div className="ck-inbox-actions">
                  {prompt && prompt.choices.length > 0 ? (
                    prompt.choices.map((ch, i) => (
                      <button
                        key={ch.id}
                        type="button"
                        className={`ck-btn ${i === 0 ? 'ck-btn--primary' : 'ck-btn--secondary'}`}
                        onClick={() => void c.answer(id, prompt.promptId, ch.id, ch.label)}
                      >
                        {ch.label}
                      </button>
                    ))
                  ) : null}
                  <button type="button" className="ck-btn ck-btn--plain" onClick={() => c.select(id)}>
                    Open terminal
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
