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

export interface McpBackendStatus {
  backend: string;
  label: string;
  installed: boolean;
  usesNpm: boolean;
  configPath: string;
  message: string;
}

export interface McpStatusReport {
  bridgeReachable: boolean;
  bridgeUrl: string | null;
  bridgeVersion: string | null;
  portFileExists: boolean;
  portFilePath: string;
  nodeAvailable: boolean;
  npmAvailable: boolean;
  npmPackageVersion: string | null;
  launchCommand: string;
  backends: McpBackendStatus[];
  overallReady: boolean;
  repairResults: EnsureMcpResult[];
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

export async function uninstallNpmMcpConfigs(projectPath: string): Promise<EnsureMcpResult[]> {
  return tauri.uninstallNpmMcpConfigs(projectPath);
}

export async function uninstallGlobalNpmMcpConfigs(): Promise<EnsureMcpResult[]> {
  return tauri.uninstallGlobalNpmMcpConfigs();
}

export async function getMcpStatus(
  projectPath: string,
  autoRepair = false,
): Promise<McpStatusReport> {
  return tauri.getMcpStatus(projectPath, autoRepair);
}

export function isOrchestratorMcpBackend(
  backend: OrchestratorBackend,
): backend is CliOrchestratorBackend {
  return isCliOrchestratorBackend(backend);
}
