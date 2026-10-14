// Test helpers for the Settings sections and controls (no testing-library in this repo: act + createRoot).
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { vi } from 'vitest';
import type { Settings } from '@puppet-master/shared';
import type { SettingsPatch } from '../../lib/settings';
import type { RowDef, SettingsEnv } from './model';
import { SectionRows } from './SectionView';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

export const BASE_SETTINGS: Settings = {
  default_provider: 'anthropic',
  default_model: 'claude-sonnet-4-6',
  custom_models: [],
  orchestrator_backend: 'api',
  mobile_input_delay_ms: 60,
  mobile_input_visible: true,
  mobile_keyboard_resizes_agent: false,
  dev_server_port: 1420,
  sidebar_width: 360,
  theme: 'dark',
  ui_mode: 'orchestration',
  developer_use_rust_mcp: false,
  confirm_close_running_pane: true,
  checkpoints_enabled: true,
  checkpoints_keep: 50,
  away_minutes: 5,
  budget_default_turns: 0,
  budget_default_minutes: 0,
};

export interface TestEnv extends SettingsEnv {
  /** Every patch the rows saved, in order. */
  patches: SettingsPatch[];
  patch: ReturnType<typeof vi.fn<(patch: SettingsPatch) => Promise<void>>>;
  markSaved: ReturnType<typeof vi.fn<() => void>>;
  setError: ReturnType<typeof vi.fn<(message: string | null) => void>>;
  confirm: ReturnType<typeof vi.fn<SettingsEnv['confirm']>>;
}

/** An env whose `patch` records what was saved and applies it to `settings`, like the panel does. */
export function makeEnv(overrides: Partial<SettingsEnv> = {}): TestEnv {
  const patches: SettingsPatch[] = [];
  const env = {
    settings: { ...BASE_SETTINGS },
    projectPath: 'C:\\work\\demo',
    bridgeUrl: 'http://127.0.0.1:17321',
    update: {},
    ...overrides,
    patches,
    patch: vi.fn(async (patch: SettingsPatch) => {
      patches.push(patch);
      for (const [k, v] of Object.entries(patch)) {
        if (v === null || v === undefined) delete (env.settings as unknown as Record<string, unknown>)[k];
        else (env.settings as unknown as Record<string, unknown>)[k] = v;
      }
    }),
    markSaved: vi.fn(),
    setError: vi.fn(),
    confirm: vi.fn(async () => true),
  } as TestEnv;
  return env;
}

export interface Mounted {
  host: HTMLDivElement;
  env: TestEnv;
  /** Re-render with the env's current settings (after a patch). */
  rerender: () => Promise<void>;
  unmount: () => Promise<void>;
}

/** Mount rows inside the same list markup the panel uses. */
export async function mountRows(rows: RowDef[], overrides: Partial<SettingsEnv> = {}): Promise<Mounted> {
  const host = document.createElement('div');
  document.body.appendChild(host);
  const root: Root = createRoot(host);
  const env = makeEnv(overrides);
  const draw = async () => {
    await act(async () => {
      root.render(<SectionRows rows={rows} envFor={() => ({ ...env, settings: { ...env.settings } })} />);
    });
    await settle();
  };
  await draw();
  return {
    host,
    env,
    rerender: draw,
    unmount: async () => {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

/** Let pending promises and effects run. */
export async function settle(times = 3): Promise<void> {
  for (let i = 0; i < times; i++) {
    await act(async () => {
      await Promise.resolve();
    });
  }
}

export async function click(el: Element | null | undefined): Promise<void> {
  if (!el) throw new Error('click: element not found');
  await act(async () => {
    (el as HTMLElement).click();
  });
}

/** Type into an input without committing (fires the input event only). */
export async function type(input: HTMLInputElement | HTMLTextAreaElement, value: string): Promise<void> {
  const proto = input instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  await act(async () => {
    Object.getOwnPropertyDescriptor(proto, 'value')!.set!.call(input, value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

export async function blur(el: HTMLElement): Promise<void> {
  await act(async () => {
    el.focus();
    el.blur();
    el.dispatchEvent(new FocusEvent('focusout', { bubbles: true }));
  });
}

export async function pickOption(select: HTMLSelectElement, value: string): Promise<void> {
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value')!.set!.call(select, value);
    select.dispatchEvent(new Event('change', { bubbles: true }));
  });
}

export const byLabel = <T extends Element = HTMLElement>(host: ParentNode, label: string): T | null =>
  host.querySelector<T>(`[aria-label="${label}"]`);

export const buttonByText = (host: ParentNode, text: string): HTMLButtonElement | undefined =>
  [...host.querySelectorAll('button')].find((b) => b.textContent?.trim() === text) as HTMLButtonElement | undefined;

export const hasText = (host: ParentNode, text: string): boolean => (host.textContent ?? '').includes(text);
