// The sandbox's status, kept fresh: read once, then again whenever the app says something changed, while an install runs
// (its events carry the bar), and every 15 seconds (the app stops an idle sandbox by itself, which is not an event).
import { useCallback, useEffect, useState } from 'react';
import { plainSandboxError, type SandboxTransport, type SandboxView } from '../../../lib/potato-sandbox';

export const POLL_MS = 15_000;

export function useSandbox(transport: SandboxTransport, options: { poll?: boolean } = {}) {
  const poll = options.poll ?? true;
  const [view, setView] = useState<SandboxView | null>(null);
  const [failed, setFailed] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setView(await transport.status());
      setFailed(null);
    } catch (error) {
      setFailed(plainSandboxError(error, 'Could not read the sandbox.'));
    }
  }, [transport]);

  useEffect(() => {
    let live = true;
    let offInstall: (() => void) | undefined;
    let offChanged: (() => void) | undefined;
    const safe = () => {
      if (live) void refresh();
    };
    safe();
    transport
      .onInstall?.((install) => {
        if (!live) return;
        setView((current) => (current ? { ...current, state: install.state === 'running' ? 'installing' : current.state, install } : current));
      })
      .then((off) => {
        if (live) offInstall = off;
        else off();
      });
    transport
      .onChanged?.(safe)
      .then((off) => {
        if (live) offChanged = off;
        else off();
      });
    const timer = poll ? setInterval(safe, POLL_MS) : undefined;
    return () => {
      live = false;
      offInstall?.();
      offChanged?.();
      if (timer) clearInterval(timer);
    };
  }, [transport, refresh, poll]);

  return { view, failed, refresh };
}
