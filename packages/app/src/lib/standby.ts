// Warm standby pool settings helpers (pure; the settings tab and its tests share them).
import type { StandbyConfig, StandbyStatus } from './tauri-types';

export type StandbyKind = 'visible' | 'headless';

export const DEFAULT_STANDBY_CONFIG: StandbyConfig = {
  enabled: false,
  backends: {},
  max_total: 3,
  recent_projects: 2,
  recent_hours: 6,
  idle_timeout_secs: 1200,
};

/** Human labels for the backends that can have a standby worker. */
export const STANDBY_BACKEND_LABELS: Record<string, string> = {
  cursor_agent: 'Cursor Agent',
  claude: 'Claude Code',
};

export function backendLabel(agentType: string): string {
  return STANDBY_BACKEND_LABELS[agentType] ?? agentType;
}

/** Is this kind of standby switched on for the backend? */
export function isBackendEnabled(config: StandbyConfig, agentType: string, kind: StandbyKind): boolean {
  return config.backends[agentType]?.[kind] ?? false;
}

/** A new config with one backend/kind toggled; the input is not mutated. */
export function withBackendToggle(
  config: StandbyConfig,
  agentType: string,
  kind: StandbyKind,
  value: boolean,
): StandbyConfig {
  const current = config.backends[agentType] ?? { visible: false, headless: false };
  return {
    ...config,
    backends: { ...config.backends, [agentType]: { ...current, [kind]: value } },
  };
}

/** Clamp an edited number into the range the backend accepts (it clamps too; this keeps the UI honest). */
export function clampInt(value: number, min: number, max: number, fallback: number): number {
  if (!Number.isFinite(value)) return fallback;
  return Math.min(max, Math.max(min, Math.round(value)));
}

export function formatDuration(ms: number | null | undefined): string {
  if (ms == null || !Number.isFinite(ms)) return '-';
  const seconds = Math.max(0, Math.round(ms / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  return minutes < 60 ? `${minutes}m ${seconds % 60}s` : `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

/** One line for the status strip: "2 standby (1 ready, 1 starting)" or why nothing is warm. */
export function summarizeStandbys(status: StandbyStatus | null): string {
  if (!status) return 'Standby pool status is unavailable outside the desktop app.';
  if (!status.enabled) return 'Off: no standby workers are kept.';
  const { total, ready, starting } = status.counts;
  if (total === 0) return 'On: no standby workers yet. One is started after your next run in a project.';
  return `${total} standby (${ready} ready, ${starting} starting)`;
}
