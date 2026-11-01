// The computer's upkeep in the Computer tab: whether the sandbox runs and how much disk it uses, Start / Stop / Restart, and
// the way to Settings > Potato Bot > Sandbox. Also the friendly "Set up the computer" card while there is none yet. All of it
// goes through the potato_sandbox_* commands (the SandboxTransport), the same ones the settings rows use.
import { useState } from 'react';
import { COMPUTER_COPY, sandboxLine } from '../../../lib/potato-computer';
import { REMOVE_WORD, plainSandboxError, type SandboxTransport, type SandboxView } from '../../../lib/potato-sandbox';
import { ConfirmSheet } from '../../settings/controls';
import '../../../styles/cockpit-potato-panel.css';

export interface ComputerMaintenanceProps {
  transport: SandboxTransport;
  view: SandboxView;
  /** Reads the status again after a call. */
  refresh: () => Promise<void>;
  onOpenSettings?: () => void;
  onNotify?: (text: string) => void;
}

type Doing = 'start' | 'stop' | 'restart';

/** Start, Stop and Restart run through here; `busy` is the word of the one that runs. */
export function useSandboxActions(transport: SandboxTransport, refresh: () => Promise<void>, onNotify?: (text: string) => void) {
  const [busy, setBusy] = useState<Doing | null>(null);
  const run = async (what: Doing) => {
    if (busy) return;
    setBusy(what);
    try {
      if (what === 'start') await transport.start();
      else if (what === 'stop') await transport.stop();
      else {
        await transport.stop();
        await transport.start();
      }
    } catch (error) {
      onNotify?.(plainSandboxError(error, 'That did not work.'));
    } finally {
      setBusy(null);
      await refresh();
    }
  };
  return { busy, start: () => run('start'), stop: () => run('stop'), restart: () => run('restart') };
}

export function ComputerMaintenance({ transport, view, refresh, onOpenSettings, onNotify }: ComputerMaintenanceProps) {
  const actions = useSandboxActions(transport, refresh, onNotify);
  const { busy } = actions;
  const running = view.state === 'running';
  const idle = view.state === 'stopped' || view.state === 'failed';
  const starting = view.state === 'starting' || busy === 'start';
  const line = busy === 'stop' ? 'Stopping…' : busy === 'restart' ? 'Restarting…' : starting ? 'Starting…' : sandboxLine(view);
  return (
    <section className="ck-potato-maint" aria-label={COMPUTER_COPY.sectionTitle}>
      <h3 className="ck-potato-maint-title">{COMPUTER_COPY.sectionTitle}</h3>
      <p className="ck-sub ck-potato-maint-line" role="status">
        {line}
      </p>
      <div className="ck-potato-maint-buttons">
        {idle && (
          <button type="button" className="ck-btn ck-btn--secondary" disabled={busy !== null} onClick={() => void actions.start()}>
            {COMPUTER_COPY.start}
          </button>
        )}
        {running && (
          <>
            <button type="button" className="ck-btn ck-btn--secondary" disabled={busy !== null} onClick={() => void actions.stop()}>
              {COMPUTER_COPY.stop}
            </button>
            <button type="button" className="ck-btn ck-btn--secondary" disabled={busy !== null} onClick={() => void actions.restart()}>
              {COMPUTER_COPY.restart}
            </button>
          </>
        )}
        {onOpenSettings && (
          <button type="button" className="ck-btn ck-btn--plain" onClick={onOpenSettings}>
            {COMPUTER_COPY.settings}
          </button>
        )}
      </div>
    </section>
  );
}

export interface SetUpCardProps {
  botName: string;
  view: SandboxView;
  onSetUp: () => void;
}

/** The sandbox is not set up (or is being set up): the call to action, in place of the preview. */
export function SetUpCard({ botName, view, onSetUp }: SetUpCardProps) {
  const setting = view.state === 'installing';
  return (
    <section className="ck-potato-setup" aria-label={COMPUTER_COPY.setUpTitle}>
      <h3 className="ck-potato-setup-title">{COMPUTER_COPY.setUpTitle}</h3>
      <p className="ck-sub ck-potato-setup-body">{setting ? COMPUTER_COPY.settingUp(view.install.percent) : COMPUTER_COPY.setUpBody(botName)}</p>
      <button type="button" className="ck-btn ck-btn--primary" onClick={onSetUp}>
        {setting ? COMPUTER_COPY.showSetUp : COMPUTER_COPY.setUpButton}
      </button>
    </section>
  );
}

export interface UnfinishedCardProps {
  transport: SandboxTransport;
  view: SandboxView;
  refresh: () => Promise<void>;
  /** Start over removed the unfinished computer: open the set up. */
  onSetUp: () => void;
  onNotify?: (text: string) => void;
}

/** Set up was cut off (the app closed mid-way): "Setup didn't finish" with Continue setup / Start over, in place of Wake. */
export function UnfinishedCard({ transport, view, refresh, onSetUp, onNotify }: UnfinishedCardProps) {
  const [busy, setBusy] = useState<'continue' | 'over' | null>(null);
  const [asking, setAsking] = useState(false);
  const go = async (what: 'continue' | 'over', call: () => Promise<unknown>, after?: () => void) => {
    setBusy(what);
    try {
      await call();
      after?.();
    } catch (error) {
      onNotify?.(plainSandboxError(error, 'That did not work.'));
    } finally {
      setBusy(null);
      await refresh();
    }
  };
  return (
    <section className="ck-potato-setup" aria-label={COMPUTER_COPY.unfinishedTitle}>
      <h3 className="ck-potato-setup-title">{COMPUTER_COPY.unfinishedTitle}</h3>
      <p className="ck-sub ck-potato-setup-body">{view.canContinue ? COMPUTER_COPY.unfinishedBody : COMPUTER_COPY.unfinishedBodyNoResume}</p>
      <div className="ck-potato-maint-buttons">
        {view.canContinue && (
          <button type="button" className="ck-btn ck-btn--primary" disabled={busy !== null} onClick={() => void go('continue', () => transport.continueInstall())}>
            {busy === 'continue' ? COMPUTER_COPY.continuing : COMPUTER_COPY.continueSetUp}
          </button>
        )}
        <button type="button" className={`ck-btn ${view.canContinue ? 'ck-btn--secondary' : 'ck-btn--primary'}`} disabled={busy !== null} onClick={() => setAsking(true)}>
          {COMPUTER_COPY.startOver}
        </button>
      </div>
      {asking && (
        <ConfirmSheet
          title={COMPUTER_COPY.startOverTitle}
          body={COMPUTER_COPY.startOverBody}
          confirmLabel={COMPUTER_COPY.startOver}
          onConfirm={() => {
            setAsking(false);
            void go('over', () => transport.remove(REMOVE_WORD), onSetUp);
          }}
          onCancel={() => setAsking(false)}
        />
      )}
    </section>
  );
}
