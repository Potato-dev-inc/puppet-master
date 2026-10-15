// The Rust attention snapshot on the desktop: read once through `attention_snapshot`, then kept
// current by `attention://changed` frames (applied only when their `rev` is newer). Returns null
// where there is no Tauri backend (tests, the harness), so callers fall back to the local derivation.
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { isTauri } from '../lib/cockpit-api';
import { newerSnapshot, type AttentionSnapshot } from '../lib/attention';

export function useAttention(): AttentionSnapshot | null {
  const [snapshot, setSnapshot] = useState<AttentionSnapshot | null>(null);
  useEffect(() => {
    if (!isTauri()) return;
    let live = true;
    let unlisten: UnlistenFn | undefined;
    const apply = (next: AttentionSnapshot) => live && setSnapshot((cur) => newerSnapshot(cur, next));
    void listen<AttentionSnapshot>('attention://changed', (e) => apply(e.payload)).then((fn) => {
      if (live) unlisten = fn;
      else fn();
    });
    invoke<AttentionSnapshot>('attention_snapshot').then(apply).catch(() => {});
    return () => {
      live = false;
      unlisten?.();
    };
  }, []);
  return snapshot;
}
