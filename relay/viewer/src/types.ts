// Wire types of the Live Room viewer. The relay only moves RoomFrame (ciphertext); Frame is what the
// key opens. Names follow SPEC-n32 3.6 and 3.7 (camelCase in JSON, like the app's LiveItem).

/** The version every room frame carries. */
export const FRAME_VERSION = 1;

export type FrameKind = 'snap' | 'delta' | 'state';

/** One encrypted frame as the relay sends it (`RoomFrame` in pm-relay-protocol). */
export interface RoomFrame {
  v: number;
  room: string;
  seq: number;
  kind: FrameKind;
  /** 12 bytes, base64url (no padding). */
  nonce: string;
  /** Ciphertext with the GCM tag, base64url (no padding). */
  ct: string;
}

/** A long-poll answer (`RoomBatch`). */
export interface RoomBatch {
  frames: RoomFrame[];
  next: number;
  ended: string | null;
}

export type LiveState = 'working' | 'idle' | 'waiting' | 'error' | 'gone';

export interface RoomInfo {
  title: string;
  endsAt: number;
  paused: boolean;
  hidden: number;
}

export interface PaneView {
  key: string;
  label: string;
  title: string;
  state: LiveState;
  line: string | null;
}

export interface TaskView {
  text: string;
  status: string | null;
}

/** One chat row, after the app's redaction (`LiveItem` in the app). */
export type ItemView =
  | { t: 'user'; id: number; pane: string; text: string }
  | { t: 'agent'; id: number; pane: string; text: string }
  | { t: 'tool'; id: number; pane: string; name: string; summary: string; files: number; writes: boolean }
  | { t: 'result'; id: number; pane: string; ok: boolean; lines: number }
  | { t: 'end'; id: number; pane: string; outcome: string; ms: number | null }
  | { t: 'plan'; id: number; pane: string; tasks: TaskView[]; replace: boolean }
  | { t: 'note'; id: number; pane: string; text: string };

/** The decrypted JSON of one frame (SPEC-n32 3.6). Every part is optional on the wire. */
export interface Frame {
  v: number;
  kind: FrameKind;
  room?: RoomInfo;
  panes?: PaneView[];
  items?: ItemView[];
}
