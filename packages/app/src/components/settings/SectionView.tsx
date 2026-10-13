// Renders rows as grouped inset lists. Used by the panel for a section page and for search results,
// and by the section tests (which pass their own `envFor`).
import type { ReactNode } from 'react';
import { groupRows, type RowDef, type SettingsEnv } from './model';

export interface RowChrome {
  /** Row ids whose "Saved" tick is showing. */
  saved?: ReadonlySet<string>;
  /** Row id -> error line. */
  errors?: Readonly<Record<string, string>>;
}

/** One row: the row's own markup, plus the quiet "Saved" tick and its error line. */
export function RowShell({ row, env, chrome }: { row: RowDef; env: SettingsEnv; chrome?: RowChrome }): ReactNode {
  const on = chrome?.saved?.has(row.id) ?? false;
  const error = chrome?.errors?.[row.id];
  return (
    <div className="st-rowwrap" data-row={row.id}>
      {row.render(env)}
      <span className={`st-saved${on ? ' is-on' : ''}`} role="status" aria-live="polite">
        {on ? 'Saved' : ''}
      </span>
      {error && (
        <p className="st-rowerr" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}

/** A section's rows as groups: headed lists first, then the destructive rows in their own last list. */
export function SectionRows({
  rows,
  envFor,
  chrome,
}: {
  rows: RowDef[];
  envFor: (row: RowDef) => SettingsEnv;
  chrome?: RowChrome;
}) {
  return (
    <>
      {groupRows(rows).map((group, i) => (
        <section key={`${group.head ?? ''}-${i}`} className="st-group" data-danger={group.danger ? 'true' : undefined}>
          {group.head && <h2>{group.head}</h2>}
          <div className="st-list">
            {group.rows.map((row) => (
              <RowShell key={row.id} row={row} env={envFor(row)} chrome={chrome} />
            ))}
          </div>
        </section>
      ))}
    </>
  );
}
