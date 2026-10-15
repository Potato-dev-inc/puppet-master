import { describe, expect, it } from 'vitest';
import { SettingsSchema } from './schemas';

describe('SettingsSchema ui_mode', () => {
  it('defaults to orchestration', () => {
    expect(SettingsSchema.parse({}).ui_mode).toBe('orchestration');
  });
  it('accepts individual and rejects unknown modes', () => {
    expect(SettingsSchema.parse({ ui_mode: 'individual' }).ui_mode).toBe('individual');
    expect(SettingsSchema.safeParse({ ui_mode: 'grid' }).success).toBe(false);
  });
});
