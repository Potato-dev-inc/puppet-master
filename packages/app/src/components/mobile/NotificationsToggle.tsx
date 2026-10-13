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
    <div className="border-b border-pm-border py-3">
      <label className="flex items-center justify-between gap-2 text-pm-muted">
        <span className="text-pm-text">Notifications</span>
        <input
          type="checkbox"
          aria-label="Notifications"
          checked={view.checked}
          disabled={view.disabled || (!bridgeReady && state.kind !== 'on')}
          onChange={(e) => void toggle(e.target.checked)}
          className="h-4 w-4 accent-pm-accent"
        />
      </label>
      {view.hint && (
        <p role={state.kind === 'error' ? 'alert' : undefined} className="mt-1 text-[11px] text-pm-muted/80">
          {view.hint}
        </p>
      )}
    </div>
  );
}
