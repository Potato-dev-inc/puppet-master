// @vitest-environment jsdom
// Settings > Bridge & MCP > Security: the two switches save through the app's own commands and the
// audit list is read-only.
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { click, mountRows, settle } from '../test-utils';
import { securityRows, whenText } from './bridge/Security';

const mocks = vi.hoisted(() => ({
  state: { ask_before_agent_control: false, agents_may_merge: true },
  entries: [
    { seq: 2, ts_ms: Date.now() - 120_000, kind: 'consent', who: 'desktop', summary: 'You allowed Claude Code to control Codex, once' },
    { seq: 1, ts_ms: Date.now() - 3_600_000, kind: 'approval', who: 'device', summary: 'A phone answered from its lock screen: Allowed once' },
  ],
  verify: { ok: true, entries: 2, broken: null as null | { file: string; line: number; reason: string } },
}));

vi.mock('../../../lib/agent-authority-api', async (importOriginal) => {
  const real = await importOriginal<typeof import('../../../lib/agent-authority-api')>();
  return {
    ...real,
    agentAuthorityApi: {
      get: vi.fn(async () => ({ ...mocks.state })),
      set: vi.fn(async (patch: Partial<typeof mocks.state>) => {
        mocks.state = { ...mocks.state, ...patch };
        return { ...mocks.state };
      }),
      auditRecent: vi.fn(async () => mocks.entries),
      auditVerify: vi.fn(async () => mocks.verify),
    },
  };
});

import { agentAuthorityApi } from '../../../lib/agent-authority-api';

const ASK = "Ask me before an agent controls a pane it didn't start";
const MERGE = 'Let agents merge separate copies';
const switchFor = (host: HTMLElement, label: string) => host.querySelector<HTMLElement>(`[aria-label="${label}"]`);

beforeEach(() => {
  mocks.state = { ask_before_agent_control: false, agents_may_merge: true };
  mocks.verify = { ok: true, entries: 2, broken: null };
  vi.clearAllMocks();
});

describe('Security rows', () => {
  it("shows today's behaviour by default: A off, B on", async () => {
    const m = await mountRows(securityRows);
    await settle(5);
    expect(switchFor(m.host, ASK)?.getAttribute('aria-checked')).toBe('false');
    expect(switchFor(m.host, MERGE)?.getAttribute('aria-checked')).toBe('true');
    await m.unmount();
  });

  it('saves a switch through the app command and keeps the other one', async () => {
    const m = await mountRows(securityRows);
    await settle(5);
    await click(switchFor(m.host, ASK));
    await settle(5);
    expect(agentAuthorityApi.set).toHaveBeenCalledWith({ ask_before_agent_control: true });
    expect(mocks.state).toEqual({ ask_before_agent_control: true, agents_may_merge: true });
    await click(switchFor(m.host, MERGE));
    await settle(5);
    expect(mocks.state).toEqual({ ask_before_agent_control: true, agents_may_merge: false });
    expect(switchFor(m.host, MERGE)?.getAttribute('aria-checked')).toBe('false');
    await m.unmount();
  });

  it('puts a switch back and says so when the save fails', async () => {
    vi.mocked(agentAuthorityApi.set).mockRejectedValueOnce(new Error('disk full'));
    const m = await mountRows(securityRows);
    await settle(5);
    await click(switchFor(m.host, MERGE));
    await settle(5);
    expect(switchFor(m.host, MERGE)?.getAttribute('aria-checked')).toBe('true');
    expect(m.env.setError).toHaveBeenCalledWith('Not saved: disk full');
    await m.unmount();
  });

  it('lists recent approvals read-only, newest first, in plain words', async () => {
    const m = await mountRows(securityRows);
    await settle(5);
    const rows = [...m.host.querySelectorAll('.st-audit-row')];
    expect(rows.map((r) => r.querySelector('.st-audit-what')?.textContent)).toEqual([
      'You allowed Claude Code to control Codex, once',
      'A phone answered from its lock screen: Allowed once',
    ]);
    expect(rows.map((r) => r.querySelector('.st-audit-who')?.textContent)).toEqual(['You', 'A paired phone']);
    expect(m.host.querySelector('.st-audit')?.querySelectorAll('button, input, select')).toHaveLength(0);
    expect(m.host.textContent).toContain('Log checked: 2 entries, none changed.');
    await m.unmount();
  });

  it('warns when the log was changed', async () => {
    mocks.verify = { ok: false, entries: 1, broken: { file: '2026-10.jsonl', line: 2, reason: 'the line was changed after it was written' } };
    const m = await mountRows(securityRows);
    await settle(5);
    const alert = m.host.querySelector('.st-audit-check[role="alert"]');
    expect(alert?.textContent).toContain('This log was changed after it was written');
    await m.unmount();
  });

  it('formats times as the time today and the date before', () => {
    const noon = new Date(2026, 9, 14, 12, 0).getTime();
    expect(whenText(new Date(2026, 9, 14, 9, 5).getTime(), noon)).toBe('09:05');
    expect(whenText(new Date(2026, 9, 13, 23, 30).getTime(), noon)).toMatch(/13/);
  });
});
