// "Check my setup": runs every check and returns the rows in order. A check that cannot read its status shows
// "Can't check this yet" (warn) instead of failing the whole page. Only existing Tauri commands are used.
import { tauriAcpAddonApi } from '../acpAddon';
import { agentHealthApi } from '../agent-health';
import { secretsApi } from '../secrets';
import { tauriSandboxTransport } from '../potato-sandbox';
import { checkAddons } from './addons';
import { checkAgents } from './agents';
import { checkBridge } from './bridge';
import { checkComputer } from './computer';
import { checkDeepseekKey } from './deepseek';
import { checkLabs } from './labs';
import type { CheckRow, SetupCheckContext, SetupCheckDeps } from './types';

export type { CheckRow, CheckStatus, FixAction, FixKind, SetupCheckContext, SetupCheckDeps } from './types';
export { buildReport, redactReport } from './report';

export const tauriSetupDeps: SetupCheckDeps = {
  agentHealth: () => agentHealthApi.check(),
  // Only `set` is read from the answer; the masked tail is never shown.
  deepseekKeySaved: async () => (await secretsApi.getMasked('deepseek_api_key')).set,
  sandboxState: async () => (await tauriSandboxTransport.status()).state,
  addons: () => tauriAcpAddonApi.list(),
};

export const CANT_CHECK_YET = "Can't check this yet";

/** The row a check shows when its status could not be read. */
export function cantCheck(id: string): CheckRow {
  return { id, status: 'warn', sentence: CANT_CHECK_YET, fix: null };
}

async function step(id: string, run: () => Promise<CheckRow[]> | CheckRow[]): Promise<CheckRow[]> {
  try {
    return await run();
  } catch {
    return [cantCheck(id)];
  }
}

export async function runAllChecks(deps: SetupCheckDeps, ctx: SetupCheckContext): Promise<CheckRow[]> {
  const rows: CheckRow[] = [];
  rows.push(...(await step('bridge', () => checkBridge(ctx))));
  rows.push(...(await step('agents', async () => checkAgents(await deps.agentHealth()))));
  rows.push(...(await step('deepseek-key', async () => checkDeepseekKey(await deps.deepseekKeySaved()))));
  if (ctx.labs.potato_sandbox_enabled === true) {
    rows.push(...(await step('sandbox', async () => checkComputer(await deps.sandboxState()))));
  }
  if (ctx.labs.acp_chat_enabled === true) {
    rows.push(...(await step('addons', async () => checkAddons(await deps.addons()))));
  }
  rows.push(...(await step('labs', () => checkLabs(ctx))));
  return rows;
}
