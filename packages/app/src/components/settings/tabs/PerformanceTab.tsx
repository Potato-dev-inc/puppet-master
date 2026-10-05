// Settings > Performance: the opt-in warm standby pool (workers started ahead of the next run).
import { useCallback, useEffect, useState } from 'react';
import { tauri, type StandbyConfig, type StandbyStatus } from '../../../lib/tauri';
import {
  DEFAULT_STANDBY_CONFIG,
  backendLabel,
  clampInt,
  formatDuration,
  isBackendEnabled,
  summarizeStandbys,
  withBackendToggle,
  type StandbyKind,
} from '../../../lib/standby';
import { FieldInput, FieldLabel, SettingBlock, SettingToggle, SettingsSection } from '../settings-ui';

const KINDS: { kind: StandbyKind; label: string }[] = [
  { kind: 'visible', label: 'Visible panes' },
  { kind: 'headless', label: 'Headless' },
];

export function PerformanceTab() {
  const [status, setStatus] = useState<StandbyStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setStatus(await tauri.standbyGetStatus());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 3000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const config: StandbyConfig = status?.config ?? DEFAULT_STANDBY_CONFIG;
  const save = async (next: StandbyConfig) => {
    setError(null);
    try {
      setStatus(await tauri.standbySetConfig(next));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };
  const setNumber = (key: 'max_total' | 'recent_projects' | 'recent_hours', min: number, max: number, raw: string) =>
    void save({ ...config, [key]: clampInt(Number(raw), min, max, config[key]) });

  return (
    <SettingsSection
      title="Performance"
      description="Warm standby workers: a CLI for a project you keep using is started ahead of time, so the first turn of the next run skips its startup. Off by default; each standby is a real CLI process that uses memory until it is used or times out."
    >
      <SettingToggle
        label="Keep standby workers"
        description="Start one hidden worker per enabled backend for your most recently used projects. A run claims a matching one and shows it like any pane; a replacement is started afterwards. Turning this off shuts every standby down."
        checked={config.enabled}
        implemented
        onChange={(value) => void save({ ...config, enabled: value })}
      />
      <SettingBlock
        label="Backends"
        implemented
        description="Choose which backends may have a standby. Cursor headless cannot be pre-launched (it takes its prompt as an argument). Visible standbys only start in folders the CLI already trusts: the pool never answers a trust prompt."
      >
        <div className="space-y-2">
          {(status?.supported ?? []).map((backend) => (
            <div key={backend.agent_type} className="flex flex-wrap items-center gap-4 text-sm">
              <span className="w-32 font-medium">{backendLabel(backend.agent_type)}</span>
              {KINDS.map(({ kind, label }) => (
                <label key={kind} className="flex items-center gap-2 text-pm-muted">
                  <input
                    type="checkbox"
                    disabled={!backend[kind]}
                    checked={isBackendEnabled(config, backend.agent_type, kind)}
                    onChange={(event) => void save(withBackendToggle(config, backend.agent_type, kind, event.target.checked))}
                  />
                  {label}
                  {!backend[kind] && <span className="text-xs">(not supported)</span>}
                </label>
              ))}
            </div>
          ))}
          {!status && <p className="text-sm text-pm-muted">Available in the desktop app.</p>}
        </div>
      </SettingBlock>
      <SettingBlock
        label="Limits"
        implemented
        description={`Idle standbys are shut down after ${formatDuration(config.idle_timeout_secs * 1000)} and stay off until your next run in that project.`}
      >
        <div className="grid max-w-xl grid-cols-3 gap-3">
          <div>
            <FieldLabel>Max standbys</FieldLabel>
            <FieldInput type="number" min={0} max={8} value={config.max_total} onChange={(e) => setNumber('max_total', 0, 8, e.target.value)} />
          </div>
          <div>
            <FieldLabel>Recent projects</FieldLabel>
            <FieldInput type="number" min={1} max={5} value={config.recent_projects} onChange={(e) => setNumber('recent_projects', 1, 5, e.target.value)} />
          </div>
          <div>
            <FieldLabel>Used within (hours)</FieldLabel>
            <FieldInput type="number" min={1} max={336} value={config.recent_hours} onChange={(e) => setNumber('recent_hours', 1, 336, e.target.value)} />
          </div>
        </div>
      </SettingBlock>
      <SettingBlock label="Status" implemented description={summarizeStandbys(status)}>
        <div className="space-y-2 text-sm">
          {error && <p className="text-red-400">{error}</p>}
          {status?.standbys.map((standby) => (
            <div key={standby.id} className="flex flex-wrap items-center gap-x-4 gap-y-1 rounded-lg border border-pm-border px-3 py-2">
              <span className="font-medium">
                {backendLabel(standby.agent_type)}
                {standby.headless ? ' (headless)' : ''}
              </span>
              <span className="font-mono text-xs text-pm-muted">{standby.project}</span>
              <span>{standby.state}</span>
              <span className="text-pm-muted">age {formatDuration(standby.age_ms)}</span>
              {standby.idle_expires_in_ms != null && (
                <span className="text-pm-muted">expires in {formatDuration(standby.idle_expires_in_ms)}</span>
              )}
            </div>
          ))}
          {status?.blocked.map((blocked) => (
            <p key={blocked.key} className="text-xs text-pm-muted">
              Held back: {blocked.key} ({blocked.reason}
              {blocked.cold ? ', waits for your next run' : `, retry in ${formatDuration(blocked.retry_in_ms)}`})
            </p>
          ))}
          {status && (
            <p className="text-xs text-pm-muted">
              claimed {status.stats.claimed} · missed {status.stats.missed} · discarded {status.stats.discarded} · expired {status.stats.expired}
            </p>
          )}
        </div>
      </SettingBlock>
    </SettingsSection>
  );
}
