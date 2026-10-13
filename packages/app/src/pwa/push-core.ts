// Pure push logic shared by the service worker (src/sw.ts) and the tests. No DOM or worker types
// here, so it compiles in both the app and the service worker projects.

export interface PushAct {
  /** Path of the Allow / Deny route, relative to the PWA origin. */
  url: string;
  /** One-shot token bound to one prompt on one pane. */
  token: string;
}

export interface PushPayload {
  kind: string;
  title: string;
  body: string;
  tag: string;
  pane_id?: string;
  prompt_id?: string;
  act?: PushAct;
}

export interface NotificationPlan {
  title: string;
  options: {
    body: string;
    tag: string;
    renotify: boolean;
    icon: string;
    badge: string;
    data: { pane_id?: string; act?: PushAct };
    actions?: { action: 'allow' | 'deny'; title: string }[];
  };
}

const FALLBACK: PushPayload = {
  kind: 'unknown',
  title: 'Puppet Master',
  body: 'Open to see what changed',
  tag: 'pm-generic',
};

function str(value: unknown, max: number): string | undefined {
  return typeof value === 'string' && value.length > 0 ? value.slice(0, max) : undefined;
}

/** Never throws: a payload we cannot read still shows a generic notification (iOS requires one). */
export function parsePushPayload(text: string | null | undefined): PushPayload {
  if (!text) return FALLBACK;
  try {
    const raw = JSON.parse(text) as Record<string, unknown>;
    const actRaw = raw.act as Record<string, unknown> | undefined;
    const actUrl = str(actRaw?.url, 200);
    const actToken = str(actRaw?.token, 200);
    return {
      kind: str(raw.kind, 40) ?? 'unknown',
      title: str(raw.title, 120) ?? FALLBACK.title,
      body: str(raw.body, 240) ?? FALLBACK.body,
      tag: str(raw.tag, 80) ?? FALLBACK.tag,
      pane_id: str(raw.pane_id, 80),
      prompt_id: str(raw.prompt_id, 80),
      act: actUrl && actToken && actUrl.startsWith('/') ? { url: actUrl, token: actToken } : undefined,
    };
  } catch {
    return FALLBACK;
  }
}

export function planNotification(payload: PushPayload, iconBase = '/'): NotificationPlan {
  return {
    title: payload.title,
    options: {
      body: payload.body,
      tag: payload.tag,
      renotify: true,
      icon: `${iconBase}icon-192.png`,
      badge: `${iconBase}icon-192.png`,
      data: { pane_id: payload.pane_id, act: payload.act },
      actions: payload.act
        ? [
            { action: 'allow', title: 'Allow' },
            { action: 'deny', title: 'Deny' },
          ]
        : undefined,
    },
  };
}

export type ClickIntent =
  | { type: 'answer'; url: string; token: string; choice: 'allow' | 'deny' }
  | { type: 'open'; paneId: string | null };

/** What a tap on the notification (or one of its buttons) should do. */
export function clickIntent(action: string, data: { pane_id?: string; act?: PushAct } | undefined): ClickIntent {
  if ((action === 'allow' || action === 'deny') && data?.act) {
    return { type: 'answer', url: data.act.url, token: data.act.token, choice: action };
  }
  return { type: 'open', paneId: data?.pane_id ?? null };
}

/** `/?pane=<id>` for a fresh window. */
export function openUrl(paneId: string | null): string {
  return paneId ? `/?pane=${encodeURIComponent(paneId)}` : '/';
}

/** The pane a launch URL asks to open (`?pane=`), if any. */
export function paneFromSearch(search: string): string | null {
  const value = new URLSearchParams(search).get('pane');
  return value && value.length <= 80 ? value : null;
}

export const OPEN_PANE_MESSAGE = 'pm-open-pane';

export interface OpenPaneMessage {
  type: typeof OPEN_PANE_MESSAGE;
  pane_id: string;
}

export function isOpenPaneMessage(value: unknown): value is OpenPaneMessage {
  const v = value as Partial<OpenPaneMessage> | null;
  return !!v && v.type === OPEN_PANE_MESSAGE && typeof v.pane_id === 'string' && v.pane_id.length > 0;
}
