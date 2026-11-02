import { describe, expect, it } from 'vitest';
import { keySavedLine, plainError } from './model-bridge';

describe('model bridge lib', () => {
  it('turns errors into one plain sentence', () => {
    expect(plainError('This service needs the translator (coming next).')).toBe('This service needs the translator (coming next).');
    expect(plainError(new Error('Error: Save your key first.'))).toBe('Save your key first.');
    expect(plainError(undefined)).toBe('Something went wrong. Nothing was changed.');
  });
  it('says where the key is kept', () => {
    expect(keySavedLine('Windows Credential Manager')).toBe('Key saved in Windows Credential Manager.');
  });
});
