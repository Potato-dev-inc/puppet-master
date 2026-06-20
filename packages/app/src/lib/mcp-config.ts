import type { OrchestratorBackend } from '@puppet-master/shared';
import { tauri } from './tauri';
import { type CliOrchestratorBackend, isCliOrchestratorBackend } from './orchestrator-panes';

export const PUPPET_MASTER_MCP_NAME = 'puppet-master';

export const PUPPET_MASTER_MCP_COMMAND = {
  command: 'npx',
  args: ['-y', '@puppet-master/mcp'],
} as const;

export interface EnsureMcpResult {
  installed: boolean;
  changed: boolean;
  backend: string;
  message: string;
}

export async function ensureOrchestratorMcp(
  backend: CliOrchestratorBackend,
  projectPath: string,
): Promise<EnsureMcpResult> {
  return tauri.ensureOrchestratorMcp(backend, projectPath);
}

export async function installNpmMcpConfigs(projectPath: string): Promise<EnsureMcpResult[]> {
  return tauri.installNpmMcpConfigs(projectPath);
}

export async function installGlobalNpmMcpConfigs(): Promise<EnsureMcpResult[]> {
  return tauri.installGlobalNpmMcpConfigs();
}

export function isOrchestratorMcpBackend(
  backend: OrchestratorBackend,
): backend is CliOrchestratorBackend {
  return isCliOrchestratorBackend(backend);
}
