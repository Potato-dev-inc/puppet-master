// @vitest-environment node
// The phone reads "Your plans" through the PC's `/plans` route. An old PC without the route, or a refused call, gets a
// plain sentence (never a raw bridge error).
import { describe, expect, it, vi } from 'vitest';
import type { BridgeClient } from './bridge-types';
import { bridgePlansApi, type PlansReport } from './plans-report';

const REPORT = { month: { id: '2026-10' }, rows: [], totals: {}, notes: [], months: [] } as unknown as PlansReport;

describe('bridgePlansApi (the phone copy of plans_report)', () => {
  it('asks the PC for the month and returns its report as it is', async () => {
    const plansReport = vi.fn(async () => REPORT);
    const api = bridgePlansApi({ plansReport } as unknown as BridgeClient);
    await expect(api.report('2026-10')).resolves.toBe(REPORT);
    expect(plansReport).toHaveBeenCalledWith('2026-10');
    await api.report();
    expect(plansReport).toHaveBeenLastCalledWith(undefined);
  });

  it('says the PC is too old when it has no plans route', async () => {
    const api = bridgePlansApi({} as BridgeClient);
    await expect(api.report()).rejects.toThrow('This PC is too old to show your plans on the phone.');
  });

  it('reads a refused call as the PC sentence, and a network failure as a plain reach message', async () => {
    const refused = bridgePlansApi({
      plansReport: async () => {
        throw new Error('bridge GET /plans -> 403: {"error":"plans can only be read from the app and the paired phone"}');
      },
    } as unknown as BridgeClient);
    await expect(refused.report()).rejects.toThrow('plans can only be read from the app and the paired phone');
    const offline = bridgePlansApi({
      plansReport: async () => {
        throw new Error('Failed to fetch');
      },
    } as unknown as BridgeClient);
    await expect(offline.report()).rejects.toThrow('Could not reach your PC.');
  });
});
