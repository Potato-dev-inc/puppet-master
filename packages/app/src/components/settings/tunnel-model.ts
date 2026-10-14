// Pure helpers for the "Public address" section in Settings > Mobile PWA.
import type { TunnelConfigInput, TunnelConfigView, TunnelMode, TunnelStatus } from '../../lib/tauri-types';

export interface TunnelDraft {
  mode: TunnelMode;
  hostname: string;
  /** Empty = keep the stored token. */
  token: string;
  url: string;
}

export const MODE_LABELS: Record<TunnelMode, string> = {
  quick: 'Automatic (changes on restart)',
  named: 'My Cloudflare tunnel',
  custom: 'My own domain or proxy',
};

export function draftFromConfig(config: TunnelConfigView | null | undefined): TunnelDraft {
  return {
    mode: config?.mode ?? 'quick',
    hostname: config?.hostname ?? '',
    token: '',
    url: config?.url ?? '',
  };
}

/** Whether Save makes sense, with the reason shown under the form when it does not. */
export function draftProblem(draft: TunnelDraft, config: TunnelConfigView | null | undefined): string | null {
  if (draft.mode === 'named') {
    if (!draft.hostname.trim()) return 'Enter the hostname, for example pm.example.com.';
    const hasStoredToken = config?.mode === 'named' && config.has_token;
    if (!draft.token.trim() && !hasStoredToken) return 'Paste the tunnel token from Cloudflare.';
  }
  if (draft.mode === 'custom' && !draft.url.trim()) return 'Enter the address, for example https://pm.example.com.';
  return null;
}

export function toInput(draft: TunnelDraft): TunnelConfigInput {
  if (draft.mode === 'named') {
    const token = draft.token.trim();
    return { mode: 'named', hostname: draft.hostname.trim(), token: token || null };
  }
  if (draft.mode === 'custom') return { mode: 'custom', url: draft.url.trim() };
  return { mode: 'quick' };
}

export interface StatusLine {
  tone: 'ok' | 'wait' | 'bad' | 'idle';
  text: string;
}

export function statusLine(status: TunnelStatus | null): StatusLine {
  if (!status) return { tone: 'idle', text: 'Available in the desktop app.' };
  const overridden = status.mode === 'env' ? ' (set outside the app)' : '';
  switch (status.state) {
    case 'running':
      return { tone: 'ok', text: `Online${overridden}` };
    case 'starting':
      return { tone: 'wait', text: 'Starting…' };
    case 'error':
      return { tone: 'bad', text: status.error ?? 'The tunnel stopped.' };
    default:
      return { tone: status.error ? 'bad' : 'idle', text: status.error ?? 'Not running.' };
  }
}

export function isStableMode(mode: TunnelStatus['mode'] | undefined): boolean {
  return mode === 'named' || mode === 'custom' || mode === 'env';
}
