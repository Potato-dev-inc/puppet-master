// The detail sheet for one Agent Store entry: who makes it, how it signs in, the exact install and run
// commands, and plain Add, Remove and Uninstall steps. The Store never types into a pane and never reads a
// credential. Catalog text is data: it is shown as plain text, and a link is shown in full and opens only
// through the app's external-link helper.
import { useCallback, useEffect, useState, type ReactNode } from 'react';
import { agentInstallApi, type AgentInstallApi, type InstallPlan } from '../../lib/agent-install';
import { agentStoreApi, type AgentStoreApi, type StoreDetail } from '../../lib/agent-store';
import { chatWords, licenseWords, primaryButton, tierWords } from '../../lib/agent-store-model';
import { tauri } from '../../lib/tauri';
import { InstallSheet } from '../cockpit/InstallSheet';
import { Sheet } from '../cockpit/Sheet';
import { errText, useInstallChain, type ChainOutcome } from './useInstallChain';

const TITLE_ID = 'ck-store-detail-t';
const COPY_FAILED = 'Could not copy. Select the command and copy it by hand.';
/** States where Puppet Master's file for the agent is on the list. */
const ON_LIST = ['ready', 'added_no_program', 'needs_review'];

type View = 'detail' | 'install' | 'remove' | 'uninstall';

const openInBrowser = (url: string) => tauri.openExternalUrl(url);

export interface AgentDetailProps {
  id: string;
  api?: AgentStoreApi;
  installApi?: AgentInstallApi;
  /** Opens the agent in its own pane, where the person signs in. */
  onOpenAgent: (id: string) => void;
  /** Something changed on the list (added, removed, installed): the grid should reload. */
  onChanged: () => void;
  onClose: () => void;
  /** Opens the Check sheet for a ready agent. Without it the "Check it works" button is not drawn. */
  onCheck?: (id: string) => void;
  /** Opens a link in the person's browser. Defaults to the app's external-link helper. */
  openExternal?: (url: string) => Promise<void>;
}

export function AgentDetail({
  id,
  api = agentStoreApi,
  installApi = agentInstallApi,
  onOpenAgent,
  onChanged,
  onClose,
  onCheck,
  openExternal = openInBrowser,
}: AgentDetailProps) {
  const [detail, setDetail] = useState<StoreDetail | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [view, setView] = useState<View>('detail');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<ChainOutcome | null>(null);
  const [uninstallPlan, setUninstallPlan] = useState<InstallPlan | null>(null);
  const chain = useInstallChain({ store: api, install: installApi });

  const load = useCallback(() => {
    api
      .detail(id)
      .then((d) => {
        setDetail(d);
        setLoadError(null);
      })
      .catch((e) => setLoadError(errText(e)));
  }, [api, id]);

  useEffect(() => {
    load();
  }, [load]);

  /** After a change: show what happened, go back to the card, reload it, and tell the grid. */
  const changed = (outcome: ChainOutcome | null) => {
    setNotice(outcome);
    setError(null);
    setView('detail');
    setUninstallPlan(null);
    load();
    onChanged();
  };

  const fail = (e: unknown) => {
    setError(errText(e));
    setBusy(false);
  };

  const copy = (text: string) => {
    if (!navigator.clipboard) return setError(COPY_FAILED);
    navigator.clipboard.writeText(text).catch(() => setError(COPY_FAILED));
  };

  const openLink = (url: string) => {
    openExternal(url).catch((e) => setError(errText(e)));
  };

  if (chain.state.phase === 'running') {
    const run = chain.state;
    return (
      <Sheet labelledBy={TITLE_ID}>
        <h2 className="ck-section" id={TITLE_ID}>
          {run.verb} {detail?.label ?? id}
        </h2>
        <p className="ck-sub">
          Step {run.step} of {run.of}: {run.label}. A window opened. Wait until it finishes. Puppet Master checks when
          its command ends.
        </p>
      </Sheet>
    );
  }

  if (!detail) {
    return (
      <Sheet labelledBy={TITLE_ID} onClose={onClose}>
        <h2 className="ck-section" id={TITLE_ID}>
          {loadError ? 'Could not open this agent' : 'Loading'}
        </h2>
        {loadError && <p className="ck-sub ck-doctor-error">{loadError}</p>}
        <div className="ck-sheet-actions">
          <button type="button" className="ck-btn ck-btn--secondary" autoFocus onClick={onClose}>
            Close
          </button>
        </div>
      </Sheet>
    );
  }

  const name = detail.label;
  const button = primaryButton(detail);
  const onList = ON_LIST.includes(detail.state);

  const runInstall = (acknowledgedRisk: boolean) => {
    setError(null);
    setNotice(null);
    void chain
      .install({
        id,
        label: name,
        steps: detail.install_plan.steps,
        acknowledgedRisk,
        alreadyAdded: detail.state === 'added_no_program',
        ambiguous: detail.ambiguous_name,
      })
      .then(changed);
  };

  if (view === 'install') {
    return (
      <InstallSheet
        plan={detail.install_plan}
        busy={false}
        error={error}
        onRun={runInstall}
        onCancel={() => {
          setError(null);
          setView('detail');
        }}
        onCopy={copy}
      />
    );
  }

  const addToList = () => {
    setBusy(true);
    setError(null);
    api
      .add(id, true)
      .then(() => {
        setBusy(false);
        changed({ tone: 'ok', text: `Added ${name}. Open it to sign in.` });
      })
      .catch(fail);
  };

  const removeFromList = () => {
    setBusy(true);
    setError(null);
    api
      .remove(id)
      .then(() => {
        setBusy(false);
        changed({ tone: 'ok', text: `Removed ${name} from Puppet Master. Settings > Agents can put it back for a while.` });
      })
      .catch(fail);
  };

  const askUninstall = () => {
    setBusy(true);
    setError(null);
    api
      .uninstallPlan(id)
      .then((plan) => {
        setBusy(false);
        if (!plan.available || plan.steps.length !== 1) {
          setError(plan.reason ?? 'This cannot be removed from here.');
          return;
        }
        setUninstallPlan(plan);
        setView('uninstall');
      })
      .catch(fail);
  };

  const runUninstall = (plan: InstallPlan) => {
    setError(null);
    setNotice(null);
    void chain.uninstall({ id, label: name, step: plan.steps[0] }).then(changed);
  };

  return (
    <Sheet labelledBy={TITLE_ID} onClose={busy ? undefined : onClose}>
      <h2 className="ck-section" id={TITLE_ID}>
        {name}
      </h2>
      <p className="ck-sub">{detail.tagline}</p>
      <p className="ck-sub">
        <span className="ck-chip">
          <span className="ck-chip-name">{tierWords(detail)}</span>
        </span>{' '}
        <span className="ck-chip">
          <span className="ck-chip-name">{licenseWords(detail)}</span>
        </span>
      </p>
      <p className="ck-sub">{detail.state_reason}</p>
      {notice && <p className={`ck-sub${notice.tone === 'problem' ? ' ck-doctor-error' : ''}`}>{notice.text}</p>}
      {error && <p className="ck-sub ck-doctor-error">{error}</p>}
      {loadError && <p className="ck-sub ck-doctor-error">{loadError}</p>}

      <Fact label="Vendor">{detail.vendor.name}</Fact>
      <Fact label="Homepage">
        <LinkButton url={detail.vendor.homepage} onOpen={openLink} />
      </Fact>
      <Fact label="License">
        {detail.vendor.license}
        {detail.vendor.license_url && (
          <>
            {' '}
            <LinkButton url={detail.vendor.license_url} onOpen={openLink} />
          </>
        )}
      </Fact>
      {detail.vendor.terms_url && (
        <Fact label="Terms">
          <LinkButton url={detail.vendor.terms_url} onOpen={openLink} />
        </Fact>
      )}
      <Fact label="Sign in">
        {detail.sign_in.summary} Sign-in happens in the agent's own pane. Puppet Master never sees your password or key.
      </Fact>
      <Fact label="Chat">{chatWords(detail)}</Fact>
      {detail.modes.length > 0 && <Fact label="Modes">{detail.modes.join(', ')}</Fact>}
      <Fact label="Checked">
        Last checked {detail.checked.date} on version {detail.checked.version}.
        {detail.last_check &&
          ` Our check: ${detail.last_check.passed} of ${detail.last_check.total} checks passed on version ${detail.last_check.version}.`}
      </Fact>
      {!detail.checked.windows_tested && <p className="ck-sub">We have not run this on Windows ourselves.</p>}
      {detail.installed_version && <Fact label="Installed version">{detail.installed_version}</Fact>}
      {detail.found_path_display && <Fact label="Found at">{detail.found_path_display}</Fact>}
      {detail.status === 'deprecated' && (
        <p className="ck-sub">This entry is no longer updated. Its replacement is {detail.replaced_by || 'not named yet'}.</p>
      )}
      {detail.run_preview && (
        <>
          <p className="ck-sub">When you open {name}, Puppet Master starts it with this command:</p>
          <code className="ck-doctor-cmd" data-testid="run-command">
            {detail.run_preview.join(' ')}
          </code>
        </>
      )}

      {view === 'remove' && (
        <>
          <p className="ck-sub">
            Remove from Puppet Master deletes Puppet Master's file for {name} in your agents folder. The program stays on
            this PC. You can put the file back from Settings &gt; Agents for a while.
          </p>
          <div className="ck-sheet-actions">
            <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={() => setView('detail')}>
              Cancel
            </button>
            <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={removeFromList}>
              Remove from Puppet Master
            </button>
          </div>
        </>
      )}

      {view === 'uninstall' && uninstallPlan && (
        <>
          <p className="ck-sub">Puppet Master runs this command in a window you can watch:</p>
          <code className="ck-doctor-cmd" data-testid="uninstall-command">
            {uninstallPlan.steps[0]?.command}
          </code>
          <p className="ck-sub">
            This removes the {name} program only. Sign-in data and sessions it kept are not removed. Puppet Master's file
            for {name} stays until you remove it.
          </p>
          <div className="ck-sheet-actions">
            <button
              type="button"
              className="ck-btn ck-btn--secondary"
              onClick={() => {
                setUninstallPlan(null);
                setView('detail');
              }}
            >
              Cancel
            </button>
            <button type="button" className="ck-btn ck-btn--primary" onClick={() => runUninstall(uninstallPlan)}>
              Uninstall {name}
            </button>
          </div>
        </>
      )}

      {view === 'detail' && (
        <>
          {button.kind === 'install' && (
            <>
              <p className="ck-sub">
                {detail.ambiguous_name
                  ? 'Install runs the command in a window you can watch. Then you confirm the program it finds, and Puppet Master adds it.'
                  : `Install runs the command in a window you can watch. Then Puppet Master adds ${name} to your list. This is a file in your agents folder.`}
              </p>
              <div className="ck-sheet-actions">
                <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => setView('install')}>
                  Install {name}
                </button>
              </div>
            </>
          )}

          {button.kind === 'add' && detail.ambiguous_name && (
            <>
              <p className="ck-sub">
                Puppet Master found a program at {detail.found_path_display ?? 'this PC'}. Is it {name}? Press the button
                only if it is.
              </p>
              <div className="ck-sheet-actions">
                <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={addToList}>
                  Yes, add {name}
                </button>
              </div>
            </>
          )}

          {button.kind === 'add' && !detail.ambiguous_name && (
            <>
              <p className="ck-sub">
                {name} is on this PC. Adding it makes a file in your agents folder. It does not change the program.
              </p>
              <div className="ck-sheet-actions">
                <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={addToList}>
                  Add to Puppet Master
                </button>
              </div>
            </>
          )}

          {button.kind === 'open' && (
            <>
              <p className="ck-sub">Opens {name} in its own pane. Sign in there if it asks.</p>
              <div className="ck-sheet-actions">
                <button type="button" className="ck-btn ck-btn--primary" onClick={() => onOpenAgent(id)}>
                  Open {name}
                </button>
              </div>
            </>
          )}

          {button.kind === 'howto' && (
            <p className="ck-sub">
              Puppet Master cannot install {name} from here. The vendor's page has the install steps:{' '}
              <LinkButton url={detail.vendor.homepage} onOpen={openLink} />
            </p>
          )}

          {onCheck && detail.state === 'ready' && (
            <div className="ck-sheet-actions">
              <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={() => onCheck(id)}>
                Check it works
              </button>
            </div>
          )}

          {onList && (
            <p className="ck-sub">Remove from Puppet Master deletes Puppet Master's file for {name}. The program stays on this PC.</p>
          )}
          {detail.uninstall.available && (
            <p className="ck-sub">
              Uninstall the program removes {name} from this PC. Puppet Master's file stays until you remove it.
            </p>
          )}
          {(onList || detail.uninstall.available) && (
            <div className="ck-sheet-actions">
              {onList && (
                <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={() => setView('remove')}>
                  Remove from Puppet Master
                </button>
              )}
              {detail.uninstall.available && (
                <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={askUninstall}>
                  Uninstall the program
                </button>
              )}
            </div>
          )}
          {onList && !detail.uninstall.available && detail.uninstall.note && (
            <p className="ck-sub">To remove the program: {detail.uninstall.note}</p>
          )}
        </>
      )}

      {view === 'detail' && (
        <div className="ck-sheet-actions">
          <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={onClose}>
            Close
          </button>
        </div>
      )}
    </Sheet>
  );
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <p className="ck-sub">
      <b>{label}:</b> {children}
    </p>
  );
}

/** A link from the catalog. The full URL is the button text. Only https links are offered; anything else is plain text. */
function LinkButton({ url, onOpen }: { url: string; onOpen: (url: string) => void }) {
  if (!url.startsWith('https://')) return <span>{url}</span>;
  return (
    <button type="button" className="ck-btn ck-btn--plain ck-btn--sm" onClick={() => onOpen(url)}>
      {url}
    </button>
  );
}
