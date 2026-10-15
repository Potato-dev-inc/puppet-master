import { describe, expect, it } from 'vitest';
import { newerSnapshot, type AttentionSnapshot } from './attention';

const snap = (rev: number, device?: AttentionSnapshot['device']): AttentionSnapshot => ({ v: 1, rev, counts: { blocking: 0, ready: 0 }, items: [], device });
const can = { can_answer: true, can_type: false, can_control: false };

describe('newerSnapshot keeps what this phone may do', () => {
  it('applies only a newer revision', () => {
    expect(newerSnapshot(snap(5), snap(4)).rev).toBe(5);
    expect(newerSnapshot(snap(5), snap(6)).rev).toBe(6);
    expect(newerSnapshot(null, snap(1)).rev).toBe(1);
  });

  it('a broadcast frame has no device block: the last read’s stays', () => {
    expect(newerSnapshot(snap(5, can), snap(6)).device).toEqual(can);
  });

  it('a read at the same revision still updates what the phone may do (the PC can change it any time)', () => {
    const next = newerSnapshot(snap(5, can), snap(5, { can_answer: false, can_type: false, can_control: false }));
    expect(next.rev).toBe(5);
    expect(next.device?.can_answer).toBe(false);
  });

  it('an older read never goes back', () => {
    expect(newerSnapshot(snap(5, can), snap(4, { can_answer: false, can_type: false, can_control: false })).device).toEqual(can);
  });
});
