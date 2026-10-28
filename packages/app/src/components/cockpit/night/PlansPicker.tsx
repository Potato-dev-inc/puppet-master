// "Plans to use" on the Tonight sheet (SPEC n32 3.10): one row per installed agent and saved login. A plan that
// is out of usage stays enabled and shows the time; a plan that cannot run is shown with the reason and cannot be
// ticked. Two logins of one agent are one company, so ticking one unticks the other (the desktop refuses the
// rest, since the plan list has no company field).
import type { NightPlanView } from '../../../lib/night';

/** The agent part of a plan key (`agent` or `agent@account`). Two logins of one agent are one company. */
export const agentOf = (key: string): string => key.split('@')[0];

export function PlansPicker({
  plans,
  ticked,
  notice,
  onToggle,
}: {
  plans: NightPlanView[];
  ticked: string[];
  /** The plain line after a same-company untick. */
  notice: string | null;
  onToggle: (key: string, on: boolean) => void;
}) {
  return (
    <section className="ck-night-plans" aria-labelledby="ck-night-plans-t">
      <div className="ck-night-group-head">
        <span className="ck-night-label" id="ck-night-plans-t">
          Plans to use
        </span>
        <span className="ck-night-hint">(one login per company)</span>
      </div>
      {plans.length === 0 ? (
        <p className="ck-night-note">No agent is set up on this computer yet.</p>
      ) : (
        <ul className="ck-night-plan-list">
          {plans.map((plan) => {
            const on = ticked.includes(plan.key);
            const status = plan.outUntil ?? (plan.ready ? 'Ready' : (plan.whyNot ?? 'Not ready'));
            return (
              <li key={plan.key}>
                <label className="ck-night-plan">
                  <input
                    type="checkbox"
                    checked={on}
                    disabled={!plan.ready && !on}
                    onChange={(e) => onToggle(plan.key, e.target.checked)}
                  />
                  <span className="ck-night-plan-name">{plan.label}</span>
                  <span className="ck-night-plan-status">{status}</span>
                </label>
              </li>
            );
          })}
        </ul>
      )}
      {notice && (
        <p className="ck-night-note" role="status">
          {notice}
        </p>
      )}
    </section>
  );
}
