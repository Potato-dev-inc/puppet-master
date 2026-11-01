// Settings > Agents > "Check my setup": one list of plain sentences. Each row has a tick, a "!" or a cross, and at most
// one fix button. "Copy report" copies every row as plain text, after the path and key scrubber (lib/setup-checks/report).
import { useCallback, useEffect, useState } from 'react';
import { tauriAcpAddonApi } from '../../lib/acpAddon';
import { openSettingsAt } from '../../lib/openSettingsAt';
import {
  buildReport,
  runAllChecks,
  tauriSetupDeps,
  type CheckRow,
  type FixAction,
  type SetupCheckContext,
  type SetupCheckDeps,
} from '../../lib/setup-checks';
import { Btn, Note } from './settings-ui';

export interface SetupCheckActions {
  signIn: (agentId: string) => void;
  installAddon: (agentId: string) => Promise<void> | void;
  addKey: () => void;
  openSandbox: () => void;
}

/** Where each fix button goes. Settings-page defaults; tests pass fakes. */
export const DEFAULT_SETUP_ACTIONS: SetupCheckActions = {
  signIn: () => openSettingsAt('agents', 'agent-lane'),
  installAddon: (agentId) => tauriAcpAddonApi.install(agentId).then(() => undefined),
  addKey: () => openSettingsAt('potato', 'potato-key', { focusField: true }),
  openSandbox: () => openSettingsAt('potato', 'potato-sandbox'),
};

/** Runs the button's action. */
export function runFix(fix: FixAction, actions: SetupCheckActions): Promise<void> | void {
  switch (fix.kind) {
    case 'signin':
      return actions.signIn(fix.agentId ?? '');
    case 'installAddon':
      return actions.installAddon(fix.agentId ?? '');
    case 'addKey':
      return actions.addKey();
    case 'openSandbox':
      return actions.openSandbox();
  }
}

const GLYPH: Record<CheckRow['status'], string> = { ok: '✓', warn: '!', fail: '×' };

export interface SetupCheckPageProps {
  ctx: SetupCheckContext;
  deps?: SetupCheckDeps;
  actions?: SetupCheckActions;
  appVersion?: string;
  now?: () => Date;
  /** Clipboard writer; defaults to the browser clipboard. */
  copy?: (text: string) => Promise<void>;
}

export function SetupCheckPage({
  ctx,
  deps = tauriSetupDeps,
  actions = DEFAULT_SETUP_ACTIONS,
  appVersion = 'dev',
  now = () => new Date(),
  copy,
}: SetupCheckPageProps) {
  const [rows, setRows] = useState<CheckRow[] | null>(null);
  const [message, setMessage] = useState<{ tone: 'status' | 'alert'; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [tick, setTick] = useState(0);

  // The checks run on mount and on "Check again" (tick).
  useEffect(() => {
    let alive = true;
    setRows(null);
    void runAllChecks(deps, ctx).then((next) => {
      if (alive) setRows(next);
    });
    return () => {
      alive = false;
    };
    // ctx is read once per run; a new run needs "Check again".
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tick, deps]);

  const fix = useCallback(
    async (f: FixAction) => {
      setBusy(true);
      setMessage(null);
      try {
        await runFix(f, actions);
      } catch (error) {
        setMessage({ tone: 'alert', text: error instanceof Error ? error.message : 'That did not work. Try again.' });
      } finally {
        setBusy(false);
      }
    },
    [actions],
  );

  const copyReport = async () => {
    if (!rows) return;
    const text = buildReport(rows, appVersion, now().toISOString().slice(0, 16).replace('T', ' '));
    try {
      await (copy ?? ((t: string) => navigator.clipboard.writeText(t)))(text);
      setMessage({ tone: 'status', text: 'Report copied. Folder paths with a user name and keys are removed.' });
    } catch {
      setMessage({ tone: 'alert', text: 'Could not copy. Select the list and copy it by hand.' });
    }
  };

  return (
    <div className="st-setupcheck" data-testid="setup-check">
      <div className="st-setupcheck-list" role="list">
        {rows === null && <p className="st-note">Checking…</p>}
        {rows?.map((r) => (
          <div key={r.id} className="st-setupcheck-row" role="listitem" data-status={r.status}>
            <span aria-hidden="true">{GLYPH[r.status]}</span>
            <span className="st-setupcheck-text">{r.sentence}</span>
            {r.fix && (
              <Btn small disabled={busy} onClick={() => void fix(r.fix as FixAction)}>
                {r.fix.label}
              </Btn>
            )}
          </div>
        ))}
      </div>
      <div className="st-setupcheck-actions">
        <Btn small disabled={rows === null} onClick={() => void copyReport()}>
          Copy report
        </Btn>
        <Btn small disabled={rows === null} onClick={() => setTick((t) => t + 1)}>
          Check again
        </Btn>
      </div>
      {message && <Note role={message.tone}>{message.text}</Note>}
    </div>
  );
}
