// Preload (warm standby) settings helpers (pure; the Preload tab and its tests share them).
import { peekAgentRegistry, type AgentDescriptor } from './agentRegistry';
import type { StandbyConfig, StandbyStatus } from './tauri-types';

/** Most warm workers one provider may keep (mirrors the backend clamp). */
export const MAX_WARM_PER_PROVIDER = 2;

export const DEFAULT_STANDBY_CONFIG: StandbyConfig = {
  preload: { enabled: false, providers: {} },
  enabled: false,
  backends: {},
  max_total: 3,
  recent_projects: 2,
  recent_hours: 6,
  idle_timeout_secs: 1200,
};

export function backendLabel(agentType: string): string {
  return peekAgentRegistry().agents.find((agent) => agent.id === agentType)?.label ?? agentType;
}

/** One row of the provider checklist. */
export interface PreloadProvider {
  id: string;
  label: string;
  /** False: the agent can open a visible pane but has no warm mode yet (its row is disabled). */
  supported: boolean;
}

/** Agents that can open a visible pane, in registry order. */
export function preloadProviders(agents: readonly AgentDescriptor[]): PreloadProvider[] {
  return agents
    .filter((agent) => agent.modes.visible)
    .map((agent) => ({ id: agent.id, label: agent.label, supported: agent.standby.visible }));
}

/** Warm workers wanted for a provider (0 when unticked). */
export function preloadCount(config: StandbyConfig, agentId: string): number {
  return config.preload.providers[agentId] ?? 0;
}

/** A new config with one provider's warm count set (0 unticks it); the input is not mutated. */
export function withPreloadCount(config: StandbyConfig, agentId: string, count: number): StandbyConfig {
  const next = { ...config.preload.providers };
  const n = Math.min(MAX_WARM_PER_PROVIDER, Math.max(0, Math.round(Number.isFinite(count) ? count : 0)));
  if (n === 0) delete next[agentId];
  else next[agentId] = n;
  return { ...config, preload: { ...config.preload, providers: next } };
}

export function withPreloadEnabled(config: StandbyConfig, enabled: boolean): StandbyConfig {
  return { ...config, preload: { ...config.preload, enabled } };
}

/** Idle timeout is edited in minutes and stored in seconds. */
export function withIdleMinutes(config: StandbyConfig, minutes: number): StandbyConfig {
  const m = clampInt(minutes, 1, 24 * 60, Math.round(config.idle_timeout_secs / 60));
  return { ...config, idle_timeout_secs: m * 60 };
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

/** One line for the status strip: "2 warm (1 ready, 1 starting)" or why nothing is warm. */
export function summarizePreload(status: StandbyStatus | null): string {
  if (!status) return 'Preload status is unavailable outside the desktop app.';
  if (!status.config.preload.enabled) return 'Off: no agents are kept warm.';
  const { total, ready, starting } = status.counts;
  if (Object.keys(status.config.preload.providers).length === 0) return 'On, but no provider is ticked.';
  if (total === 0) return 'On: warm agents start in a few seconds.';
  return `${total} warm (${ready} ready, ${starting} starting)`;
}
