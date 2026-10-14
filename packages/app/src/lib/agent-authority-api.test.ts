import { describe, expect, it } from 'vitest';
import { whoText, type AuditEntry } from './agent-authority-api';

describe('whoText', () => {
  it('says who did it in words', () => {
    const who: AuditEntry['who'][] = ['desktop', 'device', 'agent', 'teammate', 'local'];
    expect(who.map(whoText)).toEqual(['You', 'A paired phone', 'An agent', 'A teammate', 'A program on this computer']);
  });

  it('accepts the team audit kinds', () => {
    const kinds: AuditEntry['kind'][] = ['team.sent', 'team.received', 'team.action', 'team.joined', 'team.role_changed', 'team.key_rotated'];
    expect(kinds).toHaveLength(6);
  });
});
