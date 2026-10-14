import { teamApi, type TeamApi } from '../../../lib/team';
import { TellTeamPrompt } from './TellTeam';
import { TeamCard } from './TeamCard';
import { TeamJobCard } from './TeamJobCard';
import { useTeam } from './useTeam';
import '../../../styles/cockpit-team.css';

/**
 * Inbox › Team: below "Needs you". My unsent "Tell the team?" drafts, my queued updates and their
 * results, and teammates' cards. Renders nothing while Team is off (Settings › General › Labs).
 * Team cards do not add to the Needs-you count: they wait for me but block no agent.
 */
export function TeamSection({ api = teamApi, now, paneNames }: { api?: TeamApi; now: number; paneNames?: Map<string, string> }) {
  const { state, refresh } = useTeam(api);
  if (!state?.enabled) return null;
  const jobs = state.jobs.filter((j) => j.state !== 'discarded');
  const empty = state.drafts.length === 0 && jobs.length === 0 && state.cards.length === 0;

  return (
    <section className="ck-team" aria-label="Team">
      <h2 className="ck-caption ck-team-caption">
        Team
        {!state.active && <span className="ck-sub-inline"> · finish setting it up in Settings › General</span>}
      </h2>
      {state.error && (
        <p className="ck-team-error" role="alert">
          {state.error}
        </p>
      )}
      {state.transport === 'relay' && state.link === 'offline' && (
        <p className="ck-sub ck-team-empty">Can't reach the team's relay. Updates will arrive when it's back.</p>
      )}
      {state.missing > 0 && (
        <div className="ck-team-gap">
          <span className="ck-sub">Some team updates may be missing.</span>
          <button
            type="button"
            className="ck-btn ck-btn--plain"
            onClick={() => {
              api
                .clearGaps()
                .catch(() => {})
                .finally(refresh);
            }}
          >
            OK
          </button>
        </div>
      )}
      {empty && state.active && <p className="ck-sub ck-team-empty">When a teammate pushes, it shows up here.</p>}

      {state.drafts.length > 0 && (
        <ul className="ck-list ck-team-list" aria-label="Unsent">
          {state.drafts.map((d) => (
            <li key={d.id} className="ck-inbox-item">
              <TellTeamPrompt draft={d} members={state.members} api={api} variant="row" onDone={refresh} />
            </li>
          ))}
        </ul>
      )}

      {jobs.length > 0 && (
        <ul className="ck-list ck-team-list" aria-label="Updates for your branches">
          {jobs.map((j) => (
            <TeamJobCard key={j.id} job={j} api={api} paneName={j.paneId ? paneNames?.get(j.paneId) : undefined} onDone={refresh} />
          ))}
        </ul>
      )}

      {state.cards.length > 0 && (
        <ul className="ck-list ck-team-list" aria-label="From your team">
          {state.cards.map((card) => (
            <TeamCard key={card.id} card={card} api={api} now={now} onDone={refresh} />
          ))}
        </ul>
      )}
    </section>
  );
}
