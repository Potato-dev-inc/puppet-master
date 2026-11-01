// The chat add-ons of every agent: read once, kept fresh by the `acp-addon-changed` event (and a slow poll while one is
// installing, in case an event is missed). Install and remove go through the typed commands; a failure becomes the
// agent's `failed` status with the backend's plain sentence.
import { useCallback, useEffect, useRef, useState } from 'react';
import { plainMessage, tauriAcpAddonApi, type AcpAddonApi, type AddonStatus } from '../lib/acpAddon';

const POLL_MS = 2000;

export interface AcpAddons {
  /** Chat (ACP) is on in Labs and the app can install add-ons. */
  labsEnabled: boolean;
  /** False until the first read has finished (or failed). */
  loaded: boolean;
  byAgent: Record<string, AddonStatus>;
  /** Why the last removal of an agent's add-on did not happen (the add-on is still installed). */
  removeErrors: Record<string, string>;
  install: (agentId: string) => Promise<void>;
  remove: (agentId: string) => Promise<void>;
  refresh: () => Promise<void>;
}

export function useAcpAddons(api: AcpAddonApi = tauriAcpAddonApi, enabled = true): AcpAddons {
  const [labsEnabled, setLabs] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [byAgent, setBy] = useState<Record<string, AddonStatus>>({});
  const [removeErrors, setRemoveErrors] = useState<Record<string, string>>({});
  const live = useRef(true);

  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);

  const refresh = useCallback(async () => {
    try {
      const list = await api.list();
      if (!live.current) return;
      setLabs(list.labsEnabled);
      setBy(Object.fromEntries(list.addons.map((a) => [a.agent, a])));
    } catch {
      /* the page works without it */
    } finally {
      if (live.current) setLoaded(true);
    }
  }, [api]);

  const put = useCallback((status: AddonStatus) => {
    if (live.current) setBy((all) => ({ ...all, [status.agent]: status }));
  }, []);

  const failed = useCallback((agentId: string, error: unknown) => {
    const detail = plainMessage(error instanceof Error ? error.message : typeof error === 'string' ? error : 'Something went wrong.');
    if (!live.current) return;
    setBy((all) => {
      const was = all[agentId];
      return was ? { ...all, [agentId]: { ...was, state: 'failed', detail, lines: [] } } : all;
    });
  }, []);

  useEffect(() => {
    if (!enabled) return undefined;
    let off: (() => void) | null = null;
    let gone = false;
    api
      .watch(put)
      .then((stop) => (gone ? stop() : (off = stop)))
      .catch(() => undefined);
    void refresh();
    return () => {
      gone = true;
      off?.();
    };
  }, [api, enabled, put, refresh]);

  const anyInstalling = Object.values(byAgent).some((a) => a.state === 'installing');
  useEffect(() => {
    if (!enabled || !anyInstalling) return undefined;
    const timer = window.setInterval(() => void refresh(), POLL_MS);
    return () => window.clearInterval(timer);
  }, [enabled, anyInstalling, refresh]);

  const install = useCallback(
    async (agentId: string) => {
      setBy((all) => (all[agentId] ? { ...all, [agentId]: { ...all[agentId], state: 'installing', detail: '', lines: [] } } : all));
      try {
        put(await api.install(agentId));
      } catch (error) {
        failed(agentId, error);
      }
    },
    [api, put, failed],
  );

  const remove = useCallback(
    async (agentId: string) => {
      setRemoveErrors(({ [agentId]: _gone, ...rest }) => rest);
      try {
        put(await api.remove(agentId));
      } catch (error) {
        const why = plainMessage(error instanceof Error ? error.message : typeof error === 'string' ? error : 'Something went wrong.');
        if (live.current) setRemoveErrors((all) => ({ ...all, [agentId]: why }));
      }
    },
    [api, put],
  );

  return { labsEnabled, loaded, byAgent, removeErrors, install, remove, refresh };
}
