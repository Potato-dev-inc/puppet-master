import { describe, expect, it } from 'vitest';
import { isNgrokHost, ngrokRequestHeaders } from './bridge-ngrok';

describe('isNgrokHost', () => {
  it('detects ngrok hostnames', () => {
    expect(isNgrokHost('https://abc123.ngrok-free.app/bridge')).toBe(true);
  });

  it('ignores local bridge URLs', () => {
    expect(isNgrokHost('http://127.0.0.1:17321')).toBe(false);
  });
});

describe('ngrokRequestHeaders', () => {
  it('adds skip-browser-warning for ngrok', () => {
    expect(ngrokRequestHeaders('https://abc.ngrok-free.app')).toEqual({
      'ngrok-skip-browser-warning': '1',
    });
  });

  it('returns empty object for local URLs', () => {
    expect(ngrokRequestHeaders('http://127.0.0.1:1420/bridge')).toEqual({});
  });
});
