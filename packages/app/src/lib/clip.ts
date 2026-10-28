// Typed wrappers for the clip commands (`commands/clip.rs`): build the plan, stage the video
// bytes, save to a path the person picked. Only the desktop window can call them; there is no
// phone or bridge route. The webview never gets raw terminal bytes: `clipPlan` returns a plan
// that Rust already redacted.
import { invoke } from '@tauri-apps/api/core';
import type { ClipPlan } from './clipTypes';

/** The video formats a clip can be saved as. mp4 is the first choice; webm is the fallback. */
export type ClipExt = 'mp4' | 'webm';

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function call<T>(command: string, args?: Record<string, unknown> | Uint8Array, options?: { headers: Record<string, string> }): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<T>(command, args, options);
}

/** Build the redacted plan for the turn that `userItemId` starts (the last turn when omitted). */
export function clipPlan(paneId: string, userItemId?: string): Promise<ClipPlan> {
  return call<ClipPlan>('clip_plan', { paneId, userItemId: userItemId ?? null });
}

/** Stage the video bytes under a plan token. The body is raw bytes; the token and ext travel in headers. Resolves to the byte count kept. */
export function clipStage(token: string, ext: ClipExt, bytes: Uint8Array): Promise<number> {
  return call<number>('clip_stage', bytes, { headers: { 'x-clip-token': token, 'x-clip-ext': ext } });
}

/** Save the staged video to a path from the system save dialog. Resolves to the saved path. */
export function clipSave(token: string, path: string): Promise<string> {
  return call<string>('clip_save', { token, path });
}

/** The default file name: only the date and the time, e.g. `clip-2026-10-28-1530.mp4` (local time). */
export function clipFileName(now: Date, ext: ClipExt = 'mp4'): string {
  const pad = (n: number): string => String(n).padStart(2, '0');
  const stamp = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}`;
  return `clip-${stamp}.${ext}`;
}
