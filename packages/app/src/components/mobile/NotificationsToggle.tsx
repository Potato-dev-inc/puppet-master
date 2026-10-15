// "Notifications" row of the phone menu sheet. State machine lives in lib/push-client.ts.
import { useEffect, useMemo, useState } from 'react';
import {
  browserDeps,
  disable,
  enable,
  initialState,
  readEnv,
  toggleView,
  type PushDeps,
  type ToggleState,
} from '../../lib/push-client';

export function NotificationsToggle({
  baseUrl,
  bridgeReady,
  deps: injected,
}: {
  baseUrl: string;
  bridgeReady: boolean;
  /** Tests inject fakes. */
  deps?: PushDeps;
}) {
  const deps = useMemo(() => injected ?? browserDeps(baseUrl), [injected, baseUrl]);
  const [state, setState] = useState<ToggleState>({ kind: 'working' });

  useEffect(() => {
    let live = true;
    const env = readEnv();
    const permission = typeof Notification === 'undefined' ? 'default' : deps.permission();
    const early = initialState(env, permission, false);
    if (early.kind !== 'off') {
      setState(early);
      return;
    }
    void deps
      .isSubscribed()
      .catch(() => false)
      .then((subscribed) => {
        if (live) setState(initialState(env, permission, subscribed));
      });
    return () => {
      live = false;
    };
  }, [deps]);

  // "Hide details on lock screen": this phone's own switch, kept on the PC. Off by default.
  const [hideDetails, setHideDetails] = useState<boolean | null>(null);
  const [hideError, setHideError] = useState(false);
  useEffect(() => {
    if (state.kind !== 'on' || !deps.getHideDetails) return;
    let live = true;
    void deps
      .getHideDetails()
      .then((hide) => live && setHideDetails(hide))
      .catch(() => live && setHideDetails(false));
    return () => {
      live = false;
    };
  }, [state.kind, deps]);
  const changeHide = async (next: boolean) => {
    if (!deps.setHideDetails) return;
    const before = hideDetails;
    setHideDetails(next);
    setHideError(false);
    try {
      setHideDetails(await deps.setHideDetails(next));
    } catch {
      setHideDetails(before ?? false);
      setHideError(true);
    }
  };

  const view = toggleView(state);
  const toggle = async (next: boolean) => {
    // No await before enable(): the permission prompt needs the tap's user activation.
    const pending = next ? enable(deps) : disable(deps);
    setState({ kind: 'working' });
    setState(await pending);
  };

  return (
    <>
      <div className="ph-group">
        <label className="ph-switch-row">
          <span className="ph-rowtext">Notifications</span>
          <input
            type="checkbox"
            aria-label="Notifications"
            className="ph-switch"
            checked={view.checked}
            disabled={view.disabled || (!bridgeReady && state.kind !== 'on')}
            onChange={(e) => void toggle(e.target.checked)}
          />
        </label>
      </div>
      {state.kind === 'on' && deps.setHideDetails && hideDetails !== null && (
        <>
          <div className="ph-group">
            <label className="ph-switch-row">
              <span className="ph-rowtext">Hide details on lock screen</span>
              <input
                type="checkbox"
                aria-label="Hide details on lock screen"
                className="ph-switch"
                checked={hideDetails}
                onChange={(e) => void changeHide(e.target.checked)}
              />
            </label>
          </div>
          <p role={hideError ? 'alert' : undefined} className={`ph-foot${hideError ? ' ph-foot--err' : ''}`}>
            {hideError
              ? "Couldn't change that. Your PC didn't answer."
              : hideDetails
                ? 'Notifications only say which agent needs you. The command, the question and your names stay off the lock screen.'
                : 'Notifications can show what an agent wants to do. Turn this on to keep that off the lock screen.'}
          </p>
        </>
      )}
      {view.hint && (
        <p
          role={state.kind === 'error' ? 'alert' : undefined}
          className={`ph-foot${state.kind === 'error' ? ' ph-foot--err' : ''}`}
        >
          {view.hint}
        </p>
      )}
    </>
  );
}
