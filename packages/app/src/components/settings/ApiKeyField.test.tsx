// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { SecretMask, SecretStorageInfo, SecretsApi } from '../../lib/secrets';
import { redactSettings } from '../../lib/secrets';
import { ApiKeyField } from './ApiKeyField';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SECURE: SecretStorageInfo = { secure: true, label: 'Windows Credential Manager' };
const FULL_KEY = 'sk-ant-api03-abcdefghijklmnop-WXYZ';

function mockApi(info: SecretStorageInfo, initial: SecretMask) {
  let mask = initial;
  const api = {
    storageInfo: vi.fn(async () => info),
    getMasked: vi.fn(async () => mask),
    set: vi.fn(async (_n: string, v: string) => {
      mask = { set: true, last4: v.slice(-4) };
    }),
    remove: vi.fn(async () => {
      mask = { set: false, last4: null };
    }),
  };
  return api satisfies SecretsApi;
}

let host: HTMLDivElement;
let root: Root;

async function render(node: React.ReactNode) {
  await act(async () => {
    root.render(node);
  });
  await act(async () => {
    await Promise.resolve();
  });
}

const button = (label: string) =>
  [...host.querySelectorAll('button')].find((b) => b.textContent === label) as HTMLButtonElement | undefined;

async function click(el: Element | null | undefined) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
}

async function type(input: HTMLInputElement, value: string) {
  await act(async () => {
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
    setter.call(input, value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

async function blur(input: HTMLInputElement) {
  await act(async () => {
    input.focus();
    input.blur();
    input.dispatchEvent(new FocusEvent('focusout', { bubbles: true }));
  });
}

async function pressEnter(input: HTMLInputElement) {
  await act(async () => {
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
  });
}

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

describe('ApiKeyField', () => {
  it('shows a stored key masked with where it is stored, and never the full key', async () => {
    const api = mockApi(SECURE, { set: true, last4: 'WXYZ' });
    await render(<ApiKeyField label="Anthropic API key" name="anthropic_api_key" placeholder="sk-ant" api={api} />);
    const state = host.querySelector('[data-testid="key-state"]')!.textContent;
    expect(state).toBe('•••• WXYZ · Stored in Windows Credential Manager');
    expect(host.querySelector('input')).toBeNull();
    expect(host.textContent).not.toContain('abcdefghijklmnop');
    expect(button('Replace')).toBeTruthy();
    expect(button('Remove')).toBeTruthy();
  });

  it('saves a new key through the store on blur, then shows it masked', async () => {
    const api = mockApi(SECURE, { set: false, last4: null });
    const onSaved = vi.fn();
    await render(<ApiKeyField label="Anthropic API key" name="anthropic_api_key" placeholder="sk-ant" api={api} onSaved={onSaved} />);
    expect(button('Save')).toBeUndefined();
    const input = host.querySelector<HTMLInputElement>('input')!;
    await type(input, FULL_KEY);
    expect(api.set).not.toHaveBeenCalled(); // nothing is saved while typing
    await blur(input);
    expect(api.set).toHaveBeenCalledExactlyOnceWith('anthropic_api_key', FULL_KEY);
    expect(host.querySelector('[data-testid="key-state"]')!.textContent).toContain('WXYZ');
    expect(host.textContent).not.toContain('abcdefghijklmnop');
    expect(onSaved).toHaveBeenCalledTimes(1);
  });

  it('saves on Enter, and a blur afterwards does not save the same key twice', async () => {
    const api = mockApi(SECURE, { set: false, last4: null });
    await render(<ApiKeyField label="K" name="openai_api_key" placeholder="sk" api={api} />);
    const input = host.querySelector<HTMLInputElement>('input')!;
    await type(input, FULL_KEY);
    await pressEnter(input);
    await blur(input);
    expect(api.set).toHaveBeenCalledTimes(1);
  });

  it('a blur with an empty field saves nothing', async () => {
    const api = mockApi(SECURE, { set: false, last4: null });
    await render(<ApiKeyField label="K" name="openai_api_key" placeholder="sk" api={api} />);
    await blur(host.querySelector<HTMLInputElement>('input')!);
    expect(api.set).not.toHaveBeenCalled();
  });

  it('Remove asks first when asked, and keeps the key when the person says no', async () => {
    const api = mockApi(SECURE, { set: true, last4: 'WXYZ' });
    const onConfirmRemove = vi.fn(async () => false);
    await render(<ApiKeyField label="K" name="openai_api_key" placeholder="sk" api={api} onConfirmRemove={onConfirmRemove} />);
    await click(button('Remove'));
    expect(onConfirmRemove).toHaveBeenCalledTimes(1);
    expect(api.remove).not.toHaveBeenCalled();
    expect(host.querySelector('[data-testid="key-state"]')!.textContent).toContain('WXYZ');
  });

  it('Remove deletes the stored key once the person confirms', async () => {
    const api = mockApi(SECURE, { set: true, last4: 'WXYZ' });
    const onSaved = vi.fn();
    await render(<ApiKeyField label="K" name="openai_api_key" placeholder="sk" api={api} onConfirmRemove={async () => true} onSaved={onSaved} />);
    await click(button('Remove'));
    expect(api.remove).toHaveBeenCalledWith('openai_api_key');
    expect(onSaved).toHaveBeenCalledTimes(1);
  });

  it('Cancel while replacing discards what was typed without saving it', async () => {
    const api = mockApi(SECURE, { set: true, last4: 'WXYZ' });
    await render(<ApiKeyField label="K" name="openai_api_key" placeholder="sk" api={api} />);
    await click(button('Replace'));
    await type(host.querySelector<HTMLInputElement>('input')!, FULL_KEY);
    await click(button('Cancel'));
    expect(api.set).not.toHaveBeenCalled();
    expect(button('Save')).toBeUndefined();
    expect(host.querySelector('[data-testid="key-state"]')!.textContent).toContain('WXYZ');
  });

  it('Replace opens an empty field and Remove deletes the stored key', async () => {
    const api = mockApi(SECURE, { set: true, last4: 'WXYZ' });
    await render(<ApiKeyField label="K" name="openai_api_key" placeholder="sk" api={api} />);
    await click(button('Replace'));
    expect(host.querySelector<HTMLInputElement>('input')!.value).toBe('');
    await click(button('Cancel'));
    await click(button('Remove'));
    expect(api.remove).toHaveBeenCalledWith('openai_api_key');
    expect(host.querySelector('[data-testid="key-state"]')!.textContent).toContain('Not set');
  });

  it('without a credential store it warns plainly and never claims secure storage', async () => {
    const api = mockApi({ secure: false, label: 'memory' }, { set: false, last4: null });
    const onFile = vi.fn();
    await render(<ApiKeyField label="K" name="openai_api_key" placeholder="sk" api={api} fileValue={FULL_KEY} onFileChange={onFile} />);
    const text = host.textContent ?? '';
    expect(text).toContain('Secure storage is not available');
    expect(text).toContain('plain settings file (not secure)');
    expect(text).not.toMatch(/Stored in/);
    await click(button('Remove'));
    expect(onFile).toHaveBeenCalledWith(undefined);
    expect(api.set).not.toHaveBeenCalled();
  });
});

describe('redactSettings (Advanced > export)', () => {
  it('never copies a secret-looking field', () => {
    const out = redactSettings({
      theme: 'dark',
      anthropic_api_key: FULL_KEY,
      openai_api_key: '',
      github_token: 'ghp_abcdefghijkl',
    });
    expect(out.theme).toBe('dark');
    expect(out.anthropic_api_key).toBe('***');
    expect('openai_api_key' in out).toBe(false);
    expect(JSON.stringify(out)).not.toContain('abcdefghijk');
  });
});
