// Dev-server URLs per pane ("Preview" chip). Backend: `pane_previews` command for the current
// lists and the `pty://preview` event for changes (see src-tauri pane_preview.rs).
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface PreviewUrl {
  url: string;
  port: number;
  /** The LAN address the server printed for the same port, when it did. */
  networkUrl?: string;
}

/** Newest first, at most three per pane. */
export type PreviewMap = Record<string, PreviewUrl[]>;

export const previewLabel = (p: PreviewUrl): string => `:${p.port}`;

/** The preview to offer for a pane (the newest), or null. */
export function mainPreview(map: PreviewMap | undefined, paneId: string): PreviewUrl | null {
  return map?.[paneId]?.[0] ?? null;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** Opens in the user's own browser through the existing `open_external_url` command. */
export function openPreview(url: string): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>('open_external_url', { url });
}

export function usePanePreviews(): PreviewMap {
  const [map, setMap] = useState<PreviewMap>({});
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let off: (() => void) | undefined;
    invoke<PreviewMap>('pane_previews')
      .then((initial) => !disposed && setMap((cur) => ({ ...initial, ...cur })))
      .catch(() => {});
    listen<{ pane_id: string; urls: PreviewUrl[] }>('pty://preview', (e) =>
      setMap((cur) => {
        const { pane_id: id, urls } = e.payload;
        if (urls.length > 0) return { ...cur, [id]: urls };
        const { [id]: _gone, ...rest } = cur;
        return rest;
      }),
    )
      .then((u) => (disposed ? u() : (off = u)))
      .catch(() => {});
    return () => {
      disposed = true;
      off?.();
    };
  }, []);
  return map;
}
