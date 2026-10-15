// Settings > General > Budgets: the default brake for work agents start. Off by default (0): an
// agent run has no budget unless one of these is set or the agent that starts it asks for one.
// A run that reaches a limit pauses before its next turn; it is never stopped mid-turn.
import { NumberControl } from '../controls';
import type { RowDef } from '../model';
import { SettingRow } from '../settings-ui';

export const clampBudgetTurns = (n: number): number => (Number.isFinite(n) ? Math.min(10000, Math.max(0, Math.round(n))) : 0);
export const clampBudgetMinutes = (n: number): number => (Number.isFinite(n) ? Math.min(10080, Math.max(0, Math.round(n))) : 0);

const TURNS_HINT =
  'Pause an agent before its next turn after this many turns. 0 means no limit. Helpers an agent starts share this budget, up to 2 levels deep.';
const TIME_HINT = 'Pause an agent before its next turn once it has worked this long. 0 means no limit.';

export const budgetRows: RowDef[] = [
  {
    id: 'budget-turns',
    label: 'Turns an agent may use',
    hint: TURNS_HINT,
    keywords: 'budget limit cost turns pause helpers brake spend',
    group: 'Budgets',
    render: (env) => (
      <SettingRow label="Turns an agent may use" description={TURNS_HINT}>
        <NumberControl
          aria-label="Turns an agent may use (0 is no limit)"
          min={0}
          max={10000}
          value={env.settings.budget_default_turns ?? 0}
          normalize={clampBudgetTurns}
          suffix="turns"
          onCommit={(turns) => env.patch({ budget_default_turns: turns })}
        />
      </SettingRow>
    ),
  },
  {
    id: 'budget-minutes',
    label: 'Time an agent may use',
    hint: TIME_HINT,
    keywords: 'budget limit cost time minutes pause brake spend',
    group: 'Budgets',
    render: (env) => (
      <SettingRow label="Time an agent may use" description={TIME_HINT}>
        <NumberControl
          aria-label="Time an agent may use (0 is no limit)"
          min={0}
          max={10080}
          value={env.settings.budget_default_minutes ?? 0}
          normalize={clampBudgetMinutes}
          suffix="min"
          onCommit={(minutes) => env.patch({ budget_default_minutes: minutes })}
        />
      </SettingRow>
    ),
  },
];
