import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneInfo, PaneStatus } from '@puppet-master/shared';
import type { McpToolExecutor } from './mcp-tools';
import { classifyPaneAttention } from './pane-attention';
import { parseSpawnedPaneId, standIdleForWorkers } from './puppet-master';
import {
  approvePermissionIfPresent,
  CTRL_SEQUENCES,
  KEY_SEQUENCES,
  autoApprovePermissions,
  isPermissionPrompt,
  pressKey,
  wantsExplicitYes,
} from './tui-autopilot';

function pane(id: string, status: PaneStatus): PaneInfo {
  return {
    id,
    agent_type: 'bash',
    pid: 1,
    status,
    created_at: 0,
    last_output_at: null,
    cwd: '/tmp',
    cols: 80,
    rows: 24,
  };
}

/** Mock executor whose listPanes() returns successive pane snapshots. */
function makeExecutor(sequences: PaneInfo[][]): { executor: McpToolExecutor; calls: number } {
  let i = 0;
  let calls = 0;
  const executor = {
    listPanes: async (): Promise<PaneInfo[]> => {
      calls++;
      const seq = sequences[Math.min(i, sequences.length - 1)];
      i++;
      return seq;
    },
  } as unknown as McpToolExecutor;
  return { executor, calls };
}

/** Mock executor with a configurable readBuffer that returns a fixed string. */
function makeExecutorWithBuffer(
  sequences: PaneInfo[][],
  buffer: string,
): { executor: McpToolExecutor; writeInput: ReturnType<typeof vi.fn> } {
  let i = 0;
  const writeInput = vi.fn().mockResolvedValue(undefined);
  const executor = {
    listPanes: async (): Promise<PaneInfo[]> => {
      const seq = sequences[Math.min(i, sequences.length - 1)];
      i++;
      return seq;
    },
    readBuffer: async () => buffer,
    writeInput,
  } as unknown as McpToolExecutor;
  return { executor, writeInput };
}

describe('parseSpawnedPaneId', () => {
  it('parses "spawned pane" results', () => {
    expect(parseSpawnedPaneId('spawned pane: pane-1 (status=idle, ready for input)')).toBe('pane-1');
  });

  it('parses "reusing existing pane" results', () => {
    expect(parseSpawnedPaneId('reusing existing pane: pane-2 (agent=bash)')).toBe('pane-2');
  });

  it('returns null when no pane id is present', () => {
    expect(parseSpawnedPaneId('no pane here')).toBeNull();
  });
});

describe('standIdleForWorkers', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('returns all_settled immediately when tracked is empty', async () => {
    const { executor } = makeExecutor([]);
    const prev = new Map<string, PaneStatus | 'gone'>();
    const result = await standIdleForWorkers(
      executor,
      new Set(),
      prev,
      new AbortController().signal,
    );
    expect(result.reason).toBe('all_settled');
  });

  it('returns changed when seed shows an idle pane (first-seed notable — verify)', async () => {
    const { executor } = makeExecutor([[pane('a', 'idle')]]);
    const prev = new Map<string, PaneStatus | 'gone'>();
    const result = await standIdleForWorkers(
      executor,
      new Set(['a']),
      prev,
      new AbortController().signal,
    );
    expect(result.reason).toBe('changed');
    if (result.reason === 'changed') {
      expect(result.notes).toContain('pane a is idle');
    }
  });

  it('returns all_settled when previousStatus already records the pane as idle', async () => {
    const { executor } = makeExecutor([[pane('a', 'idle')]]);
    const prev = new Map<string, PaneStatus | 'gone'>([['a', 'idle']]);
    const result = await standIdleForWorkers(
      executor,
      new Set(['a']),
      prev,
      new AbortController().signal,
    );
    expect(result.reason).toBe('all_settled');
  });

  it('does not notify when seed shows a running pane (expected state)', async () => {
    const { executor } = makeExecutor([
      [pane('a', 'running')],
      [pane('a', 'running')],
      [pane('a', 'running')],
    ]);
    const prev = new Map<string, PaneStatus | 'gone'>();
    const onStandby = vi.fn();
    const promise = standIdleForWorkers(
      executor,
      new Set(['a']),
      prev,
      new AbortController().signal,
      onStandby,
      undefined,
      { pollMs: 500, maxMs: 1000 },
    );
    await vi.advanceTimersByTimeAsync(500);
    await vi.advanceTimersByTimeAsync(500);
    const result = await promise;
    expect(result.reason).toBe('timeout');
    expect(prev.get('a')).toBe('running');
    expect(onStandby).toHaveBeenCalled();
  });

  it('polls and returns changed when a running pane goes idle', async () => {
    const { executor } = makeExecutor([
      [pane('a', 'running')],
      [pane('a', 'idle')],
    ]);
    const prev = new Map<string, PaneStatus | 'gone'>();
    const promise = standIdleForWorkers(
      executor,
      new Set(['a']),
      prev,
      new AbortController().signal,
      undefined,
      undefined,
      { pollMs: 1000, maxMs: 10000 },
    );
    await vi.advanceTimersByTimeAsync(1000);
    const result = await promise;
    expect(result.reason).toBe('changed');
    if (result.reason === 'changed') {
      expect(result.notes).toContain('pane a is idle');
    }
    expect(prev.get('a')).toBe('idle');
  });

  it('reports waiting_input transitions (substantive — wakes LLM)', async () => {
    const { executor } = makeExecutorWithBuffer(
      [
        [pane('a', 'running')],
        [pane('a', 'waiting_input')],
      ],
      'Which database would you like to use? Pick one.',
    );
    const prev = new Map<string, PaneStatus | 'gone'>();
    const promise = standIdleForWorkers(
      executor,
      new Set(['a']),
      prev,
      new AbortController().signal,
      undefined,
      undefined,
      { pollMs: 500, maxMs: 5000 },
    );
    await vi.advanceTimersByTimeAsync(500);
    const result = await promise;
    expect(result.reason).toBe('changed');
    if (result.reason === 'changed') {
      expect(result.notes).toContain('pane a is waiting for input');
      expect(result.autoApproved).toEqual([]);
    }
  });

  it('reports error transitions', async () => {
    const { executor } = makeExecutor([
      [pane('a', 'running')],
      [pane('a', 'error')],
    ]);
    const prev = new Map<string, PaneStatus | 'gone'>();
    const promise = standIdleForWorkers(
      executor,
      new Set(['a']),
      prev,
      new AbortController().signal,
      undefined,
      undefined,
      { pollMs: 500, maxMs: 5000 },
    );
    await vi.advanceTimersByTimeAsync(500);
    const result = await promise;
    expect(result.reason).toBe('changed');
    if (result.reason === 'changed') {
      expect(result.notes).toContain('pane a errored');
    }
  });

  it('reports gone transitions when a pane disappears', async () => {
    const { executor } = makeExecutor([[pane('a', 'running')], []]);
    const prev = new Map<string, PaneStatus | 'gone'>();
    const promise = standIdleForWorkers(
      executor,
      new Set(['a']),
      prev,
      new AbortController().signal,
      undefined,
      undefined,
      { pollMs: 500, maxMs: 5000 },
    );
    await vi.advanceTimersByTimeAsync(500);
    const result = await promise;
    expect(result.reason).toBe('changed');
    if (result.reason === 'changed') {
      expect(result.notes).toContain('pane a exited (no longer listed)');
    }
  });

  it('reports changed when one of several panes finishes (others still running)', async () => {
    const { executor } = makeExecutorWithBuffer(
      [
        [pane('a', 'running'), pane('b', 'running')],
        [pane('a', 'idle'), pane('b', 'running')],
      ],
      '',
    );
    const prev = new Map<string, PaneStatus | 'gone'>();
    const promise = standIdleForWorkers(
      executor,
      new Set(['a', 'b']),
      prev,
      new AbortController().signal,
      undefined,
      undefined,
      { pollMs: 500, maxMs: 5000 },
    );
    await vi.advanceTimersByTimeAsync(500);
    const result = await promise;
    expect(result.reason).toBe('changed');
    if (result.reason === 'changed') {
      expect(result.notes).toEqual(['pane a is idle']);
    }
  });

  it('returns aborted when the signal is already aborted', async () => {
    const { executor } = makeExecutor([[pane('a', 'running')]]);
    const controller = new AbortController();
    controller.abort();
    const prev = new Map<string, PaneStatus | 'gone'>();
    const result = await standIdleForWorkers(
      executor,
      new Set(['a']),
      prev,
      controller.signal,
    );
    expect(result.reason).toBe('aborted');
  });

  it('returns aborted when the signal fires mid-wait', async () => {
    const { executor } = makeExecutor([[pane('a', 'running')], [pane('a', 'running')]]);
    const controller = new AbortController();
    const prev = new Map<string, PaneStatus | 'gone'>();
    const promise = standIdleForWorkers(
      executor,
      new Set(['a']),
      prev,
      controller.signal,
      undefined,
      undefined,
      { pollMs: 1000, maxMs: 10000 },
    );
    await vi.advanceTimersByTimeAsync(0);
    controller.abort();
    const result = await promise;
    expect(result.reason).toBe('aborted');
  });

  it('returns timeout when maxMs is exceeded', async () => {
    const { executor } = makeExecutor([[pane('a', 'running')]]);
    const prev = new Map<string, PaneStatus | 'gone'>();
    const promise = standIdleForWorkers(
      executor,
      new Set(['a']),
      prev,
      new AbortController().signal,
      undefined,
      undefined,
      { pollMs: 1000, maxMs: 2000 },
    );
    await vi.advanceTimersByTimeAsync(1000);
    await vi.advanceTimersByTimeAsync(1000);
    const result = await promise;
    expect(result.reason).toBe('timeout');
    if (result.reason === 'timeout') {
      expect(result.lastStatuses).toContainEqual({ id: 'a', status: 'running' });
    }
  });

  it('does not re-notify on the same idle state across successive calls', async () => {
    const prev = new Map<string, PaneStatus | 'gone'>();
    const { executor: e1 } = makeExecutor([
      [pane('a', 'running')],
      [pane('a', 'idle')],
    ]);
    const promise1 = standIdleForWorkers(
      e1,
      new Set(['a']),
      prev,
      new AbortController().signal,
      undefined,
      undefined,
      { pollMs: 500, maxMs: 5000 },
    );
    await vi.advanceTimersByTimeAsync(500);
    const r1 = await promise1;
    expect(r1.reason).toBe('changed');
    expect(prev.get('a')).toBe('idle');

    const { executor: e2 } = makeExecutor([[pane('a', 'idle')]]);
    const r2 = await standIdleForWorkers(
      e2,
      new Set(['a']),
      prev,
      new AbortController().signal,
    );
    expect(r2.reason).toBe('all_settled');
  });

  it('emits onStandby with running panes at seed and after each poll', async () => {
    const { executor } = makeExecutor([
      [pane('a', 'running'), pane('b', 'running')],
      [pane('a', 'idle'), pane('b', 'running')],
    ]);
    const prev = new Map<string, PaneStatus | 'gone'>();
    const events: Array<Array<{ id: string; status: PaneStatus }>> = [];
    const promise = standIdleForWorkers(
      executor,
      new Set(['a', 'b']),
      prev,
      new AbortController().signal,
      (running) => events.push(running),
      undefined,
      { pollMs: 500, maxMs: 5000 },
    );
    await vi.advanceTimersByTimeAsync(500);
    await promise;
    expect(events.length).toBeGreaterThanOrEqual(2);
    expect(events[0]!.map((e) => e.id).sort()).toEqual(['a', 'b']);
    expect(events[1]!.map((e) => e.id)).toEqual(['b']);
  });

  // --- Auto-approve tests (real timers — approvePermissionIfPresent has real sleeps) ---

  it('auto-approves routine permission prompts on waiting_input without waking the LLM', async () => {
    vi.useRealTimers();
    const { executor, writeInput } = makeExecutorWithBuffer(
      [
        [pane('a', 'running')],
        [pane('a', 'waiting_input')],
      ],
      'Bash command\nmkdir -p coordination-demo\nDo you want to proceed?\n❯ 1. Yes\n  2. No',
    );
    const prev = new Map<string, PaneStatus | 'gone'>();
    const onAutoApprove = vi.fn();
    const result = await standIdleForWorkers(
      executor,
      new Set(['a']),
      prev,
      new AbortController().signal,
      undefined,
      onAutoApprove,
      { pollMs: 50, maxMs: 5000 },
    );
    expect(result.reason).toBe('all_settled');
    if (result.reason === 'all_settled') {
      expect(result.autoApproved).toContain('a');
    }
    expect(onAutoApprove).toHaveBeenCalledWith('a');
    expect(writeInput).toHaveBeenCalledWith('a', '', true);
  });

  it('auto-approves explicit yes/no proceed prompts by typing y', async () => {
    vi.useRealTimers();
    const { executor, writeInput } = makeExecutorWithBuffer(
      [
        [pane('a', 'running')],
        [pane('a', 'waiting_input')],
      ],
      'Do you want to proceed with these file edits? (y/n)',
    );
    const prev = new Map<string, PaneStatus | 'gone'>();
    const onAutoApprove = vi.fn();
    const result = await standIdleForWorkers(
      executor,
      new Set(['a']),
      prev,
      new AbortController().signal,
      undefined,
      onAutoApprove,
      { pollMs: 50, maxMs: 5000 },
    );
    expect(result.reason).toBe('all_settled');
    expect(onAutoApprove).toHaveBeenCalledWith('a');
    const calls = writeInput.mock.calls;
    expect(calls.some((c) => c[1] === 'y')).toBe(true);
  });

  it('wakes the LLM when waiting_input is NOT a permission prompt', async () => {
    vi.useRealTimers();
    const { executor } = makeExecutorWithBuffer(
      [
        [pane('a', 'running')],
        [pane('a', 'waiting_input')],
      ],
      'I found 3 failing tests. Which one should I fix first?\n1) auth\n2) api\n3) db',
    );
    const prev = new Map<string, PaneStatus | 'gone'>();
    const onAutoApprove = vi.fn();
    const result = await standIdleForWorkers(
      executor,
      new Set(['a']),
      prev,
      new AbortController().signal,
      undefined,
      onAutoApprove,
      { pollMs: 50, maxMs: 5000 },
    );
    expect(result.reason).toBe('changed');
    if (result.reason === 'changed') {
      expect(result.notes).toContain('pane a is waiting for input');
      expect(result.autoApproved).toEqual([]);
    }
    expect(onAutoApprove).not.toHaveBeenCalled();
  });

  it('wakes the LLM for ambiguous OpenCode permission prompts instead of blind approval', async () => {
    vi.useRealTimers();
    const { executor, writeInput } = makeExecutorWithBuffer(
      [
        [pane('a', 'running')],
        [pane('a', 'waiting_input')],
      ],
      'Permission required\nAccess external directory ~/.Trash/\nAllow once   Allow always   Reject\nenter confirm',
    );
    const prev = new Map<string, PaneStatus | 'gone'>();
    const onAutoApprove = vi.fn();
    const result = await standIdleForWorkers(
      executor,
      new Set(['a']),
      prev,
      new AbortController().signal,
      undefined,
      onAutoApprove,
      { pollMs: 50, maxMs: 5000 },
    );
    expect(result.reason).toBe('changed');
    expect(onAutoApprove).not.toHaveBeenCalled();
    expect(writeInput).not.toHaveBeenCalled();
  });
});

describe('pressKey', () => {
  function makeExecutor(): { executor: McpToolExecutor; writeInput: ReturnType<typeof vi.fn> } {
    const writeInput = vi.fn().mockResolvedValue(undefined);
    return {
      executor: { writeInput } as unknown as McpToolExecutor,
      writeInput,
    };
  }

  it('sends the correct ANSI sequence for arrow keys', async () => {
    const { executor, writeInput } = makeExecutor();
    await pressKey(executor, 'p1', 'up');
    expect(writeInput).toHaveBeenCalledWith('p1', '\x1b[A', false);
    writeInput.mockClear();

    await pressKey(executor, 'p1', 'down');
    expect(writeInput).toHaveBeenCalledWith('p1', '\x1b[B', false);
    writeInput.mockClear();

    await pressKey(executor, 'p1', 'right');
    expect(writeInput).toHaveBeenCalledWith('p1', '\x1b[C', false);
    writeInput.mockClear();

    await pressKey(executor, 'p1', 'left');
    expect(writeInput).toHaveBeenCalledWith('p1', '\x1b[D', false);
  });

  it('sends \\r for enter', async () => {
    const { executor, writeInput } = makeExecutor();
    await pressKey(executor, 'p1', 'enter');
    expect(writeInput).toHaveBeenCalledWith('p1', '\r', false);
  });

  it('sends escape for escape', async () => {
    const { executor, writeInput } = makeExecutor();
    await pressKey(executor, 'p1', 'escape');
    expect(writeInput).toHaveBeenCalledWith('p1', '\x1b', false);
  });

  it('sends y and n for yes/no', async () => {
    const { executor, writeInput } = makeExecutor();
    await pressKey(executor, 'p1', 'y');
    expect(writeInput).toHaveBeenCalledWith('p1', 'y', false);
    writeInput.mockClear();
    await pressKey(executor, 'p1', 'n');
    expect(writeInput).toHaveBeenCalledWith('p1', 'n', false);
  });

  it('sends ctrl+c / ctrl+d / ctrl+z', async () => {
    const { executor, writeInput } = makeExecutor();
    await pressKey(executor, 'p1', 'ctrl+c');
    expect(writeInput).toHaveBeenCalledWith('p1', '\x03', false);
    writeInput.mockClear();
    await pressKey(executor, 'p1', 'ctrl+d');
    expect(writeInput).toHaveBeenCalledWith('p1', '\x04', false);
    writeInput.mockClear();
    await pressKey(executor, 'p1', 'ctrl+z');
    expect(writeInput).toHaveBeenCalledWith('p1', '\x1a', false);
  });

  it('is case-insensitive on key names', async () => {
    const { executor, writeInput } = makeExecutor();
    await pressKey(executor, 'p1', 'ENTER');
    expect(writeInput).toHaveBeenCalledWith('p1', '\r', false);
    writeInput.mockClear();
    await pressKey(executor, 'p1', 'Up');
    expect(writeInput).toHaveBeenCalledWith('p1', '\x1b[A', false);
  });

  it('returns ok with key name and byte count on success', async () => {
    const { executor } = makeExecutor();
    const r = await pressKey(executor, 'p1', 'enter');
    expect(r).toEqual({ ok: true, key: 'enter', bytes: 1 });
  });

  it('returns an error for unknown keys', async () => {
    const { executor } = makeExecutor();
    const r = await pressKey(executor, 'p1', 'frobnicate');
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error).toContain('unknown key');
  });

  it('returns an error for unsupported ctrl combinations', async () => {
    const { executor } = makeExecutor();
    const r = await pressKey(executor, 'p1', 'ctrl+q');
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error).toContain('unsupported ctrl key');
  });

  it('KEY_SEQUENCES covers all documented keys', () => {
    const expected = ['enter', 'escape', 'tab', 'space', 'up', 'down', 'left', 'right', 'home', 'end', 'pageup', 'pagedown', 'y', 'n'];
    for (const k of expected) {
      expect(KEY_SEQUENCES[k]).toBeDefined();
    }
  });

  it('CTRL_SEQUENCES covers c, d, z', () => {
    expect(CTRL_SEQUENCES.c).toBe('\x03');
    expect(CTRL_SEQUENCES.d).toBe('\x04');
    expect(CTRL_SEQUENCES.z).toBe('\x1a');
  });
});

describe('approvePermissionIfPresent', () => {
  function makeExecutor(buffer: string): { executor: McpToolExecutor; writeInput: ReturnType<typeof vi.fn> } {
    const writeInput = vi.fn().mockResolvedValue(undefined);
    return {
      executor: {
        readBuffer: async () => buffer,
        writeInput,
      } as unknown as McpToolExecutor,
      writeInput,
    };
  }

  it('returns not-prompted when buffer is not a permission prompt', async () => {
    const { executor } = makeExecutor('Running tests... 3 passed');
    const r = await approvePermissionIfPresent(executor, 'p1');
    expect(r).toBe('not-prompted');
  });

  it('returns approved and presses Enter for menu-style prompts', async () => {
    const { executor, writeInput } = makeExecutor('Permission required: Allow once / Deny');
    const r = await approvePermissionIfPresent(executor, 'p1');
    expect(r).toBe('approved');
    expect(writeInput).toHaveBeenCalledWith('p1', '', true);
  });

  it('returns approved and types y for explicit yes/no prompts', async () => {
    const { executor, writeInput } = makeExecutor('Do you want to proceed? (y/n)');
    const r = await approvePermissionIfPresent(executor, 'p1');
    expect(r).toBe('approved');
    expect(writeInput.mock.calls.some((c) => c[1] === 'y')).toBe(true);
  });

  it('returns aborted when the signal is already aborted', async () => {
    const { executor } = makeExecutor('Permission required: Allow once / Deny');
    const controller = new AbortController();
    controller.abort();
    const r = await approvePermissionIfPresent(executor, 'p1', controller.signal);
    expect(r).toBe('aborted');
  });

  it('does not auto-approve ambiguous allow-always menus', async () => {
    const { executor, writeInput } = makeExecutor('Permission required: Allow once / Allow always / Reject');
    const r = await approvePermissionIfPresent(executor, 'p1');
    expect(r).toBe('not-prompted');
    expect(writeInput).not.toHaveBeenCalled();
  });
});

describe('autoApprovePermissions', () => {
  it('does not auto-approve ambiguous allow-always menus', async () => {
    const writeInput = vi.fn().mockResolvedValue(undefined);
    const executor = {
      readBuffer: async () => 'Permission required: Allow once / Allow always / Reject',
      writeInput,
    } as unknown as McpToolExecutor;
    const result = await autoApprovePermissions(executor, 'p1', 400);
    expect(result).toBe('');
    expect(writeInput).not.toHaveBeenCalled();
  });
});

describe('classifyPaneAttention', () => {
  it('classifies Claude proceed menus as routine permission', () => {
    expect(classifyPaneAttention('Do you want to proceed?\n❯ 1. Yes\n  2. No').kind).toBe('routine_permission');
  });

  it('classifies ambiguous OpenCode permission menus as action required', () => {
    expect(
      classifyPaneAttention('Permission required\nAllow once   Allow always   Reject\nenter confirm').kind,
    ).toBe('action_required');
  });

  it('classifies worker questions as action required', () => {
    expect(classifyPaneAttention('Which database would you like to use? Pick one.').kind).toBe('action_required');
  });

  it('classifies shell failures as terminal errors', () => {
    expect(classifyPaneAttention('zsh:5: no such file or directory: coordination-demo/bin/').kind).toBe('terminal_error');
  });
});

describe('isPermissionPrompt (extended heuristics)', () => {
  it('detects "are you sure ... (y/n)" prompts', () => {
    expect(isPermissionPrompt('Are you sure you want to delete this file? (y/n)')).toBe(true);
  });

  it('detects "confirm" prompts', () => {
    expect(isPermissionPrompt('Confirm: run this command? (yes/no)')).toBe(true);
  });

  it('does not flag ordinary progress output', () => {
    expect(isPermissionPrompt('Running npm install... done')).toBe(false);
  });
});

describe('wantsExplicitYes (extended heuristics)', () => {
  it('detects "are you sure ... (y/n)" prompts', () => {
    expect(wantsExplicitYes('Are you sure you want to continue? (y/n)')).toBe(true);
  });

  it('detects "confirm ... (yes/no)" prompts', () => {
    expect(wantsExplicitYes('Confirm: proceed with deletion? yes/no')).toBe(true);
  });
});
