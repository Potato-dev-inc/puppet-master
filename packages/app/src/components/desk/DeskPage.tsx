// My computers (Labs `desk_enabled`): the page. Status line, the screen hops switch, screens, keys, hop mode.
// Nothing made up: the page shows a skeleton until the engine answers, then the real state or an honest error.
import '../../styles/cockpit-desk.css';
import { useId } from 'react';
import { useDesk, isDeskFeatureOff, type DeskHopMode } from '../../lib/desk';
import { StatusLine } from './StatusLine';
import { ScreensView } from './ScreensView';
import { KeysView } from './KeysView';

const HOP_MODES: { mode: DeskHopMode; label: string }[] = [
  { mode: 'relative', label: 'Same spot on the next screen' },
  { mode: 'memory', label: 'Where it was last time' },
  { mode: 'center', label: 'Middle of the screen' },
];

export function DeskPage() {
  const desk = useDesk();
  const hopsTitleId = useId();
  const { snapshot, loadError, actionError, reload, run } = desk;

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

          <p className="ck-desk-footnote">Coming next: share mouse and keyboard with your other computers.</p>
        </>
      )}
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
