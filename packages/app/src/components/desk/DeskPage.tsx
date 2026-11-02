// My computers (Labs `desk_enabled`): the page. Status line, the screen hops switch, screens, keys, hop mode.
// Nothing made up: the page shows a skeleton until the engine answers, then the real state or an honest error.
import '../../styles/cockpit-desk.css';
import { useId, useState } from 'react';
import { useDesk, isDeskFeatureOff, type DeskHopMode } from '../../lib/desk';
import { StatusLine } from './StatusLine';
import { ScreensView } from './ScreensView';
import { KeysView } from './KeysView';
import { LayoutEditor } from './LayoutEditor';
import { usePair } from '../../lib/desk-pair';
import { useLink } from '../../lib/desk-link';
import { useDeskPermissions } from '../../lib/desk-permissions';
import { PermissionBanner } from './PermissionBanner';
import { LinkStatus } from './LinkStatus';
import { AddComputerSheet } from './AddComputerSheet';
import { DeviceRow } from './DeviceRow';
import { InboundPairSheet } from './InboundPairSheet';
import { tauri } from '../../lib/tauri';

/** How many My computers lines the copy button takes from the app log (the newest). */
const LOG_COPY_LINES = 300;

const HOP_MODES: { mode: DeskHopMode; label: string }[] = [
  { mode: 'relative', label: 'Same spot on the next screen' },
  { mode: 'memory', label: 'Where it was last time' },
  { mode: 'center', label: 'Middle of the screen' },
];

export function DeskPage() {
  const desk = useDesk();
  const hopsTitleId = useId();
  const { snapshot, loadError, actionError, reload, run } = desk;
  const pair = usePair(snapshot !== null);
  const link = useLink(snapshot !== null);
  const access = useDeskPermissions(snapshot !== null);
  const [adding, setAdding] = useState(false);
  const [logNote, setLogNote] = useState<string | null>(null);
  const computersTitleId = useId();
  const pairView = pair.view;

  // The newest My computers lines of the app log, to paste into a bug report. Nothing leaves the computer.
  const copyLog = async () => {
    try {
      const lines = await tauri.appLogLines('desk:', LOG_COPY_LINES);
      if (lines.length === 0) {
        setLogNote('No My computers lines in the log yet.');
        return;
      }
      await navigator.clipboard.writeText(lines.join('\n'));
      setLogNote(`Copied ${lines.length} lines.`);
    } catch (e) {
      setLogNote(`Could not copy the log: ${e instanceof Error ? e.message : String(e)}`);
    }
  };

  // Labs off (or the engine says so): the page draws nothing. The left bar row is hidden too.
  if (isDeskFeatureOff(loadError) || snapshot?.status.state === 'labs_off') return null;

  const status = snapshot?.status;
  const hopsOn = status?.hops_enabled ?? false;
  const paused = status?.paused ?? false;

  return (
    <section className="ck-desk" aria-labelledby="ck-desk-title" aria-busy={!snapshot}>
      <header className="ck-desk-head">
        <h2 id="ck-desk-title" className="ck-desk-title">
          My computers
        </h2>
        {snapshot && status ? (
          <StatusLine
            status={status}
            screenCount={snapshot.screens.length}
            onFix={() => void run({ type: 'refresh' })}
          />
        ) : (
          <p className="ck-desk-status" role="status">
            {loadError ? 'Could not start screen hops.' : 'Starting…'}
          </p>
        )}
      </header>

      <PermissionBanner permissions={access.permissions} onRecheck={access.recheck} />

      <LinkStatus view={link.view} error={link.error} onPause={(p) => void link.pause(p)} onPanic={() => void link.panic()} />

      {actionError && (
        <p className="ck-desk-hint is-error" role="alert">
          Could not do that: {actionError}
        </p>
      )}

      {!snapshot && !loadError && <Skeleton />}

      {!snapshot && loadError && (
        <div className="ck-desk-empty" role="alert">
          <div className="ck-desk-stack">
            <p>Couldn't read your screens: {loadError}</p>
            <button type="button" className="ck-desk-btn" onClick={reload}>
              Try again
            </button>
          </div>
        </div>
      )}

      {snapshot && status && (
        <>
          <section className="ck-desk-section" aria-labelledby={hopsTitleId}>
            <div className="ck-desk-switch-row">
              <div className="ck-desk-stack">
                <span id={hopsTitleId} className="ck-desk-row-label">
                  Screen hops
                </span>
                <span className="ck-desk-hint">Move your mouse to another screen with a shortcut.</span>
              </div>
              <div className="ck-desk-actions">
                <button
                  type="button"
                  className="ck-desk-btn"
                  disabled={!hopsOn}
                  onClick={() => void run({ type: 'pause', paused: !paused })}
                >
                  {paused ? 'Resume' : 'Pause'}
                </button>
                <button
                  type="button"
                  role="switch"
                  aria-checked={hopsOn}
                  aria-labelledby={hopsTitleId}
                  className={`ck-desk-switch${hopsOn ? ' is-on' : ''}`}
                  onClick={() => void run({ type: 'set_hops_enabled', enabled: !hopsOn })}
                >
                  <span className="ck-desk-switch-knob" aria-hidden="true" />
                </button>
              </div>
            </div>
          </section>

          <ScreensView
            screens={snapshot.screens}
            pointer={snapshot.pointer}
            find={snapshot.find}
            onHop={(screen) => void run({ type: 'hop_to', screen })}
            onFind={() => void run({ type: 'find_pointer' })}
            onRetry={() => void run({ type: 'refresh' })}
          />

          <LayoutEditor layout={link.layout} onPlace={link.setPlace} />

          <KeysView hotkeys={snapshot.hotkeys} run={run} />

          <section className="ck-desk-section" aria-labelledby="ck-desk-hop-title">
            <h3 id="ck-desk-hop-title" className="ck-desk-h3">
              Where the pointer lands
            </h3>
            <div className="ck-desk-radios" role="radiogroup" aria-labelledby="ck-desk-hop-title">
              {HOP_MODES.map((m) => (
                <label key={m.mode} className="ck-desk-radio">
                  <input
                    type="radio"
                    name="ck-desk-hop-mode"
                    value={m.mode}
                    checked={snapshot.hop_mode === m.mode}
                    onChange={() => void run({ type: 'set_hop_mode', mode: m.mode })}
                  />
                  <span>{m.label}</span>
                </label>
              ))}
            </div>
          </section>

          <section className="ck-desk-section" aria-labelledby={computersTitleId}>
            <div className="ck-desk-section-head">
              <h3 id={computersTitleId} className="ck-desk-h3">
                Your computers
              </h3>
              <button type="button" className="ck-desk-btn" onClick={() => setAdding(true)}>
                Add a computer
              </button>
            </div>
            {pairView?.tampered && (
              <p className="ck-desk-hint is-error" role="alert">
                The saved list of computers did not pass its check, so it was not used. Pair your computers again.
              </p>
            )}
            {pairView?.message && (
              <p className="ck-desk-hint" role="status">
                {pairView.message}
              </p>
            )}
            {pair.error && !adding && (
              <p className="ck-desk-hint is-error" role="alert">
                Could not do that: {pair.error}
              </p>
            )}
            {(pairView?.devices.length ?? 0) === 0 ? (
              <p className="ck-desk-hint">Only this computer is here. Add another to share one mouse.</p>
            ) : (
              <>
              <ul className="ck-desk-devices" aria-label="Paired computers">
                {pairView?.devices.map((d) => (
                  <DeviceRow
                    key={d.node}
                    device={d}
                    link={link.view?.devices.find((l) => l.node === d.node)}
                    onSide={(side) => void link.setSide(d.node, side)}
                    onKeymap={(altCmd, winOption) => void link.setKeymap(d.node, altCmd, winOption)}
                    onScroll={(natural, speed, reverse) => void link.setScroll(d.node, natural, speed, reverse)}
                    onPointerSpeed={(speed) => void link.setPointerSpeed(d.node, speed)}
                    onToggle={(cap, on) => void pair.setPermission(d.node, cap, on)}
                    onRemove={() => void pair.remove(d.node)}
                  />
                ))}
              </ul>
              {link.view?.supported && link.view.devices.some((d) => d.input_on) && (
                <p className="ck-desk-hint">{link.view.admin_note}</p>
              )}
              </>
            )}
          </section>
        </>
      )}

      {adding && !pairView?.pending && (
        <AddComputerSheet
          hosting={pairView?.hosting ?? null}
          nearby={pair.nearby}
          joining={pairView?.joining ?? null}
          lockedUntil={pairView?.locked_until ?? null}
          onHostStart={pair.hostStart}
          onHostCancel={() => void pair.hostCancel()}
          onJoin={async (target, code) => {
            const failed = await pair.join(target, code);
            if (!failed) setAdding(false);
            return failed;
          }}
          onRefreshNearby={pair.refreshNearby}
          onClose={() => setAdding(false)}
        />
      )}
      <section className="ck-desk-section" aria-label="Diagnostics">
        <div className="ck-desk-section-head">
          <span className="ck-desk-hint">Copy the recent My computers lines from the app log for a bug report.</span>
          <button type="button" className="ck-desk-btn" onClick={() => void copyLog()}>
            Copy My computers log
          </button>
        </div>
        {logNote && (
          <p className="ck-desk-hint" role="status">
            {logNote}
          </p>
        )}
      </section>
      <InboundPairSheet pending={pairView?.pending ?? null} onAnswer={(id, allow) => void pair.answer(id, allow)} />
    </section>
  );
}

function Skeleton() {
  return (
    <div className="ck-desk-skeleton" aria-label="Loading your screens" role="img">
      <span className="ck-desk-skel" />
      <span className="ck-desk-skel is-short" />
      <span className="ck-desk-skel" />
    </div>
  );
}
