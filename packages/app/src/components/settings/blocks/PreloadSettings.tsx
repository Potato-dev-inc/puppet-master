// Settings > Preload: keep a warm agent (CLI at its input box + a hidden pane window) per ticked
// provider, so opening a pane for it is instant. Backed by the standby pool (`standby-pool.json`).
import { useCallback, useEffect, useState } from 'react';
import { tauri, type StandbyConfig, type StandbyStatus } from '../../../lib/tauri';
import { useAgentRegistry } from '../../../lib/agentRegistry';
import {
  DEFAULT_STANDBY_CONFIG,
  MAX_WARM_PER_PROVIDER,
  formatDuration,
  preloadCount,
  preloadProviders,
  summarizePreload,
  withIdleMinutes,
  withPreloadCount,
  withPreloadEnabled,
} from '../../../lib/standby';
import { NumberControl } from '../controls';
import { FieldLabel, SettingBlock, SettingToggle } from '../settings-ui';

export interface PreloadApi {
  getStatus: () => Promise<StandbyStatus | null>;
  setConfig: (config: StandbyConfig) => Promise<StandbyStatus | null>;
}

const DEFAULT_API: PreloadApi = {
  getStatus: () => tauri.standbyGetStatus(),
  setConfig: (config) => tauri.standbySetConfig(config),
};

export function PreloadSettings({ api = DEFAULT_API, onSaved }: { api?: PreloadApi; onSaved?: () => void }) {
  const registry = useAgentRegistry();
  const [status, setStatus] = useState<StandbyStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setStatus(await api.getStatus());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [api]);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 3000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const config: StandbyConfig = status?.config ?? DEFAULT_STANDBY_CONFIG;
  const save = async (next: StandbyConfig) => {
    setError(null);
    try {
      setStatus(await api.setConfig(next));
      onSaved?.();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };
  const providers = preloadProviders(registry.agents);

  return (
    <>
      <SettingToggle
        label="Preload agents"
        description="Master switch. Off by default. Each warm agent is a running CLI process that uses memory, and some providers count it as an open session. Turning this off closes every warm agent."
        checked={config.preload.enabled}
        onChange={(value) => void save(withPreloadEnabled(config, value))}
      />
      <SettingBlock
        label="Agents to keep warm"
        description="Warm agents start in your default project folder; a pane for another project starts normally. They never answer a trust or login prompt: an agent that shows one is dropped (open it once yourself first)."
      >
        <div className="space-y-2" data-testid="preload-providers">
          {providers.map((provider) => {
            const count = preloadCount(config, provider.id);
            return (
              <div key={provider.id} className="flex flex-wrap items-center gap-4 text-sm">
                <label className="flex w-56 items-center gap-2 font-medium">
                  <input
                    type="checkbox"
                    aria-label={`Keep ${provider.label} warm`}
                    disabled={!provider.supported || !status}
                    checked={count > 0}
                    onChange={(event) => void save(withPreloadCount(config, provider.id, event.target.checked ? 1 : 0))}
                  />
                  {provider.label}
                  {provider.experimental && (
                    <span
                      className="text-xs font-normal text-pm-muted"
                      title="This agent has no ready screen of its own. A warm one is used only after its screen has held still for two seconds with no dialog on it."
                    >
                      (experimental)
                    </span>
                  )}
                </label>
                {provider.supported ? (
                  <label className="flex items-center gap-2 text-pm-muted">
                    keep
                    <select
                      aria-label={`Warm ${provider.label} count`}
                      disabled={count === 0}
                      value={count || 1}
                      onChange={(event) => void save(withPreloadCount(config, provider.id, Number(event.target.value)))}
                    >
                      {Array.from({ length: MAX_WARM_PER_PROVIDER }, (_, i) => i + 1).map((n) => (
                        <option key={n} value={n}>
                          {n}
                        </option>
                      ))}
                    </select>
                    warm
                  </label>
                ) : (
                  <span className="text-xs text-pm-muted">{provider.unsupportedNote}</span>
                )}
              </div>
            );
          })}
          {!status && <p className="text-sm text-pm-muted">Available in the desktop app.</p>}
        </div>
      </SettingBlock>
      <SettingBlock
        label="Idle timeout"
        description={`A warm agent nobody used for ${formatDuration(config.idle_timeout_secs * 1000)} is closed; it warms again the next time you open that agent.`}
      >
        <div className="max-w-xs">
          <FieldLabel>Minutes</FieldLabel>
          <NumberControl
            min={1}
            max={1440}
            aria-label="Preload idle timeout (minutes)"
            value={Math.round(config.idle_timeout_secs / 60)}
            onCommit={(minutes) => save(withIdleMinutes(config, minutes))}
          />
        </div>
      </SettingBlock>
      <SettingBlock label="Status" description={summarizePreload(status)}>
        <div className="space-y-2 text-sm">
          {error && <p className="text-pm-text">{error}</p>}
          {status?.standbys
            .filter((standby) => !standby.headless)
            .map((standby) => (
              <div key={standby.id} className="flex flex-wrap items-center gap-x-4 gap-y-1 rounded-lg border border-pm-border px-3 py-2">
                <span className="font-medium">{registry.agents.find((a) => a.id === standby.agent_type)?.label ?? standby.agent_type}</span>
                <span className="font-mono text-xs text-pm-muted">{standby.project}</span>
                <span>{standby.state}</span>
                {standby.idle_expires_in_ms != null && (
                  <span className="text-pm-muted">closes in {formatDuration(standby.idle_expires_in_ms)} if unused</span>
                )}
              </div>
            ))}
          {status?.blocked.map((blocked) => (
            <p key={blocked.key} className="text-xs text-pm-muted">
              Held back: {blocked.key} ({blocked.reason}
              {blocked.cold ? ', warms again when you open it' : `, retry in ${formatDuration(blocked.retry_in_ms)}`})
            </p>
          ))}
        </div>
      </SettingBlock>
    </>
  );
}
