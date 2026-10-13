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
