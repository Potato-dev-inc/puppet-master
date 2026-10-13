// Settings > Mobile PWA > Public address: pick how the phone reaches this computer.
import { useCallback, useEffect, useState } from 'react';
import { tauri } from '../../lib/tauri';
import type { TunnelConfigInput, TunnelConnectionTest, TunnelStatus } from '../../lib/tauri-types';
import { FieldInput, FieldLabel } from './settings-ui';
import {
  MODE_LABELS,
  draftFromConfig,
  draftProblem,
  isStableMode,
  statusLine,
  toInput,
  type TunnelDraft,
} from './tunnel-model';

export interface TunnelApi {
  getStatus: () => Promise<TunnelStatus | null>;
  save: (input: TunnelConfigInput) => Promise<TunnelStatus>;
  restart: () => Promise<TunnelStatus>;
  test: () => Promise<TunnelConnectionTest>;
}

const defaultApi: TunnelApi = {
  getStatus: () => tauri.getTunnelStatus(),
  save: (input) => tauri.setTunnelConfig(input),
  restart: () => tauri.restartTunnel(),
  test: () => tauri.testTunnelConnection(),
};

const DOT: Record<string, string> = {
  ok: 'bg-neutral-200',
  wait: 'bg-neutral-500 animate-pulse',
  bad: 'border border-neutral-300 bg-transparent',
  idle: 'bg-neutral-600',
};

const buttonClass =
  'rounded-lg border border-pm-border px-3 py-1.5 text-xs hover:bg-pm-border/40 disabled:opacity-50';

export function TunnelSection({ api = defaultApi }: { api?: TunnelApi }) {
  const [status, setStatus] = useState<TunnelStatus | null>(null);
  const [draft, setDraft] = useState<TunnelDraft>(draftFromConfig(null));
  const [busy, setBusy] = useState<'save' | 'restart' | 'test' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [test, setTest] = useState<TunnelConnectionTest | null>(null);

  const load = useCallback(async () => {
    const next = await api.getStatus();
    setStatus(next);
    setDraft(draftFromConfig(next?.config));
  }, [api]);

  useEffect(() => {
    void load();
  }, [load]);

  // While the tunnel is coming up, poll so "Starting…" settles by itself.
  useEffect(() => {
    if (status?.state !== 'starting') return;
    const id = window.setInterval(() => {
      void api.getStatus().then((next) => next && setStatus(next));
    }, 1500);
    return () => window.clearInterval(id);
  }, [api, status?.state]);

  const run = async (kind: 'save' | 'restart' | 'test', action: () => Promise<void>) => {
    setBusy(kind);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };

  const problem = draftProblem(draft, status?.config);
  const line = statusLine(status);
  const dirty = JSON.stringify(draft) !== JSON.stringify(draftFromConfig(status?.config));

  return (
    <section className="rounded-xl border border-pm-border bg-pm-panel p-4" aria-label="Public address">
      <h3 className="text-sm font-semibold">Public address</h3>
      <p className="mt-1 text-sm leading-6 text-pm-muted">
        A fixed address means you pair your phone once. The automatic address changes every time the app restarts.
      </p>

      <div className="mt-3 space-y-3">
        <div>
          <FieldLabel>How your phone connects</FieldLabel>
          <select
            aria-label="Tunnel mode"
            value={draft.mode}
            onChange={(e) => setDraft({ ...draft, mode: e.target.value as TunnelDraft['mode'] })}
            className="w-full rounded-lg border border-pm-border bg-pm-bg px-3 py-2 text-sm outline-none"
          >
            {(Object.keys(MODE_LABELS) as (keyof typeof MODE_LABELS)[]).map((mode) => (
              <option key={mode} value={mode}>
                {MODE_LABELS[mode]}
              </option>
            ))}
          </select>
        </div>

        {draft.mode === 'named' && (
          <>
            <div>
              <FieldLabel>Hostname</FieldLabel>
              <FieldInput
                aria-label="Tunnel hostname"
                placeholder="pm.example.com"
                value={draft.hostname}
                onChange={(e) => setDraft({ ...draft, hostname: e.target.value })}
                className="font-mono"
              />
            </div>
            <div>
              <FieldLabel>Tunnel token</FieldLabel>
              <FieldInput
                aria-label="Tunnel token"
                type="password"
                autoComplete="off"
                placeholder={status?.config.has_token ? 'Saved. Paste a new one to replace it.' : 'Paste from Cloudflare Zero Trust'}
                value={draft.token}
                onChange={(e) => setDraft({ ...draft, token: e.target.value })}
                className="font-mono"
              />
              <p className="mt-1 text-xs text-pm-muted">
                Create a tunnel in Cloudflare Zero Trust, point its public hostname at http://127.0.0.1:1420, and copy the
                token. The token stays on this computer.
              </p>
            </div>
          </>
        )}

        {draft.mode === 'custom' && (
          <div>
            <FieldLabel>Address</FieldLabel>
            <FieldInput
              aria-label="Custom address"
              placeholder="https://pm.example.com"
              value={draft.url}
              onChange={(e) => setDraft({ ...draft, url: e.target.value })}
              className="font-mono"
            />
            <p className="mt-1 text-xs text-pm-muted">
              Point your own proxy or domain at http://127.0.0.1:1420. Puppet Master only advertises this address.
            </p>
          </div>
        )}

        <div className="flex flex-wrap items-center gap-2" aria-live="polite">
          <span className={'inline-block h-2 w-2 shrink-0 rounded-full ' + DOT[line.tone]} aria-hidden />
          <span data-testid="tunnel-status" className="text-xs text-pm-muted">
            {line.text}
          </span>
          {status?.public_url && (
            <code data-testid="tunnel-url" className="break-all text-xs">
              {status.public_url}
            </code>
          )}
          {status && !isStableMode(status.mode) && status.state === 'running' && (
            <span className="text-xs text-pm-muted">This address will change on restart.</span>
          )}
        </div>

        {(problem && dirty) || error ? (
          <p role="alert" className="text-xs text-pm-muted">
            {error ?? problem}
          </p>
        ) : null}

        {test && (
          <p data-testid="tunnel-test" className="text-xs text-pm-muted">
            {test.ok ? 'Reachable. ' : 'Not reachable. '}
            {test.message}
          </p>
        )}

        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            className={buttonClass}
            disabled={busy !== null || !dirty || problem !== null}
            onClick={() =>
              void run('save', async () => {
                const next = await api.save(toInput(draft));
                setStatus(next);
                setDraft(draftFromConfig(next.config));
                setTest(null);
              })
            }
          >
            {busy === 'save' ? 'Saving…' : 'Save and restart'}
          </button>
          <button
            type="button"
            className={buttonClass}
            disabled={busy !== null || !status}
            onClick={() =>
              void run('restart', async () => {
                setStatus(await api.restart());
                setTest(null);
              })
            }
          >
            {busy === 'restart' ? 'Restarting…' : 'Restart tunnel'}
          </button>
          <button
            type="button"
            className={buttonClass}
            disabled={busy !== null || !status?.public_url}
            onClick={() => void run('test', async () => setTest(await api.test()))}
          >
            {busy === 'test' ? 'Testing…' : 'Test connection'}
          </button>
        </div>
      </div>
    </section>
  );
}
