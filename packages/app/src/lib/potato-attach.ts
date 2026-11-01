// Sending a Potato Bot message with attached files. The brain takes text only, so the app saves the files in the bot's
// own folder and sends one text message that names them (`potato/attachments.rs`). This file is the page's side:
// reading a chosen file into memory and the one command. File contents go nowhere else. Desktop only.
import { invoke } from '@tauri-apps/api/core';
import { isTauri } from './cockpit-api';
import type { SendOutcome } from './potato';
import { COMPOSER_COPY, checkAttach, type Attachment } from './potato-composer';

/** One file on the wire (`Incoming` in attachments.rs). */
export interface WireFile {
  name: string;
  kind: 'image' | 'text';
  data: string;
}

export interface AttachTransport {
  sendWithFiles(botId: string, text: string, files: Attachment[]): Promise<SendOutcome>;
}

export const tauriAttachTransport: AttachTransport = {
  sendWithFiles: (botId, text, files) =>
    invoke<SendOutcome>('potato_send_files', { botId, text, files: files.map<WireFile>((f) => ({ name: f.name, kind: f.kind, data: f.data })) }),
};

/** The transport the page uses when none is passed in: the app's, on the desktop only. */
export function defaultAttachTransport(): AttachTransport | undefined {
  return isTauri() ? tauriAttachTransport : undefined;
}

let nextId = 1;

/** `data:image/png;base64,AAAA` -> `AAAA`. */
export function base64Of(dataUrl: string): string {
  const comma = dataUrl.indexOf(',');
  return comma >= 0 ? dataUrl.slice(comma + 1) : '';
}

function readDataUrl(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => (typeof reader.result === 'string' ? resolve(reader.result) : reject(new Error('unreadable')));
    reader.onerror = () => reject(reader.error ?? new Error('unreadable'));
    reader.readAsDataURL(file);
  });
}

export type ReadResult = { ok: true; attachment: Attachment } | { ok: false; reason: string };

/** Checks one file against the caps and the files already chosen, then reads it. A rejection says why in one sentence. */
export async function readAttachment(file: File, current: readonly Attachment[]): Promise<ReadResult> {
  const name = file.name || 'pasted file';
  const check = checkAttach(current, { name, type: file.type, size: file.size });
  if (!check.ok) return { ok: false, reason: check.reason };
  try {
    const url = await readDataUrl(file);
    return {
      ok: true,
      attachment: { id: `att-${nextId++}`, name, kind: check.kind, size: file.size, data: base64Of(url), preview: check.kind === 'image' ? url : undefined },
    };
  } catch {
    return { ok: false, reason: COMPOSER_COPY.unreadable(name) };
  }
}
