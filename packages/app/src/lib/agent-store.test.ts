import { beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

import { agentStoreApi } from './agent-store';

describe('agentStoreApi', () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockResolvedValue(undefined);
  });

  it('calls each agent_store command with the argument names Tauri expects', async () => {
    await agentStoreApi.list();
    expect(invoke).toHaveBeenLastCalledWith('agent_store_list');

    await agentStoreApi.detail('qwen');
    expect(invoke).toHaveBeenLastCalledWith('agent_store_detail', { id: 'qwen' });

    await agentStoreApi.add('qwen', true);
    expect(invoke).toHaveBeenLastCalledWith('agent_store_add', { id: 'qwen', confirmedFound: true });

    await agentStoreApi.remove('qwen');
    expect(invoke).toHaveBeenLastCalledWith('agent_store_remove', { id: 'qwen' });

    await agentStoreApi.recordInstall('qwen', 1);
    expect(invoke).toHaveBeenLastCalledWith('agent_store_record_install', { id: 'qwen', stepIndex: 1 });

    await agentStoreApi.uninstallPlan('qwen');
    expect(invoke).toHaveBeenLastCalledWith('agent_store_uninstall_plan', { id: 'qwen' });

    await agentStoreApi.uninstallRun('qwen', 0, 'npm uninstall -g @qwen-code/qwen-code');
    expect(invoke).toHaveBeenLastCalledWith('agent_store_uninstall_run', {
      id: 'qwen',
      step: 0,
      shownCommand: 'npm uninstall -g @qwen-code/qwen-code',
    });

    await agentStoreApi.recordCheck('qwen', 7, 7, '0.26.2');
    expect(invoke).toHaveBeenLastCalledWith('agent_store_record_check', {
      id: 'qwen',
      passed: 7,
      total: 7,
      version: '0.26.2',
    });
  });

  it('passes the command result back to the caller', async () => {
    invoke.mockResolvedValueOnce({ ok: true, id: 'agy', state: 'ready' });
    await expect(agentStoreApi.add('agy', false)).resolves.toEqual({ ok: true, id: 'agy', state: 'ready' });
  });
});
