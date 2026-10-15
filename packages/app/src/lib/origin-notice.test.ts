import { describe, expect, it } from 'vitest';
import { originNotice } from './origin-notice';

describe('originNotice', () => {
  it('shows the host and no warning for a normal https domain', () => {
    expect(originNotice('https://pm.example.com')).toEqual({ host: 'pm.example.com', warning: null });
  });
  it('warns on plain http to a non-local host', () => {
    expect(originNotice('http://pm.example.com')?.warning).toMatch(/plain http/);
  });
  it('warns on an IP address', () => {
    expect(originNotice('https://203.0.113.9:8443')?.warning).toMatch(/IP address/);
    expect(originNotice('http://192.168.1.4:1420')?.warning).toMatch(/plain http/);
  });
  it('warns that loopback does not work on a phone', () => {
    expect(originNotice('http://127.0.0.1:1420')?.warning).toMatch(/only works on this computer/);
  });
  it('returns null for garbage', () => {
    expect(originNotice('not a url')).toBeNull();
  });
});
