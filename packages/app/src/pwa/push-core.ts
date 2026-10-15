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
  /** The attention card the notification is about (payload v2): a tap opens `/?item=<id>`. */
  item_id?: string;
  prompt_id?: string;
  act?: PushAct;
  target?: 'questions';
}

export interface NotificationPlan {
  title: string;
  options: {
    body: string;
    tag: string;
    renotify: boolean;
    icon: string;
    badge: string;
    data: { pane_id?: string; item_id?: string; act?: PushAct; target?: 'questions' };
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
      item_id: str(raw.item_id, 40),
      prompt_id: str(raw.prompt_id, 80),
      act: actUrl && actToken && actUrl.startsWith('/') ? { url: actUrl, token: actToken } : undefined,
      target: raw.target === 'questions' ? 'questions' : undefined,
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
      data: { pane_id: payload.pane_id, item_id: payload.item_id, act: payload.act, target: payload.target },
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
  | { type: 'open'; paneId: string | null; itemId?: string | null; view?: 'questions' };

/** What a tap on the notification (or one of its buttons) should do. */
export function clickIntent(
  action: string,
  data: { pane_id?: string; item_id?: string; act?: PushAct; target?: 'questions' } | undefined,
): ClickIntent {
  if ((action === 'allow' || action === 'deny') && data?.act) {
    return { type: 'answer', url: data.act.url, token: data.act.token, choice: action };
  }
  return {
    type: 'open',
    paneId: data?.pane_id ?? null,
    itemId: data?.item_id,
    view: data?.target === 'questions' ? 'questions' : undefined,
  };
}

/** What a launch URL can ask for. The card (`?item=`) wins over the pane, the pane over the list. */
export interface OpenTarget {
  itemId?: string | null;
  paneId?: string | null;
  view?: 'questions';
}

/**
 * `/?item=<id>` for the approval card a notification announced, `/?pane=<id>` for a pane, `/?view=questions`
 * for the question list. The old call `openUrl(paneId, view)` still works.
 */
export function openUrl(target: OpenTarget | string | null, view?: 'questions'): string {
  const t: OpenTarget = typeof target === 'object' && target !== null ? target : { paneId: target as string | null, view };
  if (t.itemId) return `/?item=${encodeURIComponent(t.itemId)}`;
  if (t.paneId) return `/?pane=${encodeURIComponent(t.paneId)}`;
  return t.view === 'questions' ? '/?view=questions' : '/';
}

/** The card a launch URL asks to open (`?item=`), if any. */
export function itemFromSearch(search: string): string | null {
  const value = new URLSearchParams(search).get('item');
  return value && value.length <= 40 && /^[A-Za-z0-9_-]+$/.test(value) ? value : null;
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

/** The view a launch URL asks to open (`?view=questions`), if any. */
export function viewFromSearch(search: string): 'questions' | null {
  return new URLSearchParams(search).get('view') === 'questions' ? 'questions' : null;
}

export const OPEN_VIEW_MESSAGE = 'pm-open-view';

export interface OpenViewMessage {
  type: typeof OPEN_VIEW_MESSAGE;
  view: 'questions';
}

export function isOpenViewMessage(value: unknown): value is OpenViewMessage {
  const v = value as Partial<OpenViewMessage> | null;
  return !!v && v.type === OPEN_VIEW_MESSAGE && v.view === 'questions';
}

export const OPEN_ITEM_MESSAGE = 'pm-open-item';

export interface OpenItemMessage {
  type: typeof OPEN_ITEM_MESSAGE;
  item_id: string;
  pane_id?: string;
}

export function isOpenItemMessage(value: unknown): value is OpenItemMessage {
  const v = value as Partial<OpenItemMessage> | null;
  return !!v && v.type === OPEN_ITEM_MESSAGE && typeof v.item_id === 'string' && v.item_id.length > 0;
}

/**
 * The desktop app's own webview origin (Tauri 2: `http://tauri.localhost` on Windows,
 * `tauri://localhost` elsewhere). The service worker is for the phone PWA only; one that finds
 * itself there (registered by 0.2.0 or older) removes itself so the window loads the installed UI.
 */
export function isDesktopAppOrigin(url: { protocol: string; hostname: string }): boolean {
  return url.protocol === 'tauri:' || url.hostname === 'tauri.localhost';
}
