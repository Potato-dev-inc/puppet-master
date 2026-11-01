// @vitest-environment node
// My computers: the Labs switch is off unless it is exactly on.
import { describe, expect, it } from 'vitest';
import { deskEnabled } from './desk';

describe('deskEnabled', () => {
  it('is on only when desk_enabled is true', () => {
    expect(deskEnabled({ desk_enabled: true })).toBe(true);
    expect(deskEnabled({ desk_enabled: false })).toBe(false);
    expect(deskEnabled({})).toBe(false);
    expect(deskEnabled(null)).toBe(false);
    expect(deskEnabled({ desk_enabled: 'yes' as unknown as boolean })).toBe(false);
  });
});
