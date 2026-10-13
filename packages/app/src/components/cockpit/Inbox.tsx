import type { CockpitApi } from '../../hooks/useCockpit';
import { projectName } from '../../lib/mobile-agent-tabs';
import { DotSpinner } from './DotSpinner';
import { formatElapsed, promptSummary, spinnerStatus } from './model';
import { paneHeading } from './titles';

export function Inbox({ c }: { c: CockpitApi }) {
  const waiting = c.registry.paneList.filter((p) => p.status === 'waiting_input');
  const count = waiting.length;

  return (
    <div className="ck-inbox">
      <header className="ck-inbox-hero">
        <div className="ck-doto" aria-label={`${count} waiting`}>
          {String(count).padStart(2, '0')}
        </div>
        <h1 className="ck-title">{count === 0 ? 'Nothing needs you' : count === 1 ? 'Agent waiting for you' : 'Agents waiting for you'}</h1>
        <p className="ck-sub">
          {count === 0
            ? 'Agents that stop to ask a question will show up here.'
            : 'Answer here or open the terminal.'}
        </p>
      </header>

      {count > 0 && (
        <ul className="ck-list">
          {waiting.map((p) => {
            const id = p.info.id;
            const prompt = c.prompts[id];
            const since = c.statusSince.get(id);
            const heading = paneHeading(p.info, c.names.get(id) ?? p.info.agent_type);
            return (
              <li key={id} className="ck-inbox-item">
                <div className="ck-inbox-top">
                  <span className="ck-row-glyph">
                    <DotSpinner status={spinnerStatus(p.status)} />
                  </span>
                  <span className="ck-row-title">{heading.primary}</span>
                  <span className="ck-row-sub">
                    {heading.secondary ? `${heading.secondary} · ${projectName(p.info.cwd)}` : projectName(p.info.cwd)}
                  </span>
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
