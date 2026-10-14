// Settings > General: "Save a diagnostic file". Shows what the zip would hold before anything is written;
// the person can leave any file out and read one before saving. Nothing is uploaded.
import { useCallback, useEffect, useRef, useState } from 'react';
import { save as saveDialog } from '@tauri-apps/plugin-dialog';
import { tauri, type DiagnosticPreview } from '../../../lib/tauri';
import { Btn, Note, SettingBlock, SettingRow } from '../settings-ui';

function size(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function fileName(now = new Date()): string {
  const p = (n: number) => String(n).padStart(2, '0');
  return `puppet-master-diagnostic-${now.getFullYear()}${p(now.getMonth() + 1)}${p(now.getDate())}-${p(now.getHours())}${p(now.getMinutes())}.zip`;
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** While the list is open it re-reads itself this often, and whenever the window is focused again. */
export const DIAGNOSTIC_REFRESH_MS = 5000;

export function DiagnosticFile({ onSaved }: { onSaved?: () => void }) {
  const [preview, setPreview] = useState<DiagnosticPreview | null>(null);
  const [left, setLeft] = useState<Set<string>>(new Set());
  const [shown, setShown] = useState<{ path: string; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);

  const busyRef = useRef(false);
  busyRef.current = busy;

  const review = async () => {
    setBusy(true);
    setMessage(null);
    try {
      setPreview(await tauri.diagnosticPreview());
      setLeft(new Set());
      setShown(null);
    } catch (e) {
      setMessage({ text: `Could not look at what to include: ${errorText(e)}`, error: true });
    } finally {
      setBusy(false);
    }
  };

  // A run can fail while the list is open: keep it current without a click. What the person ticked off
  // and the file they are reading stay as they are (a path that is gone drops out of both).
  const reread = useCallback(async () => {
    if (busyRef.current) return;
    try {
      const next = await tauri.diagnosticPreview();
      setPreview(next);
      const paths = new Set(next.files.map((file) => file.path));
      setLeft((current) => new Set([...current].filter((path) => paths.has(path))));
      setShown((current) => (current && paths.has(current.path) ? current : null));
    } catch {
      // The list on screen is still what was read last; a failed re-read is not worth an error.
    }
  }, []);

  const open = preview !== null;
  useEffect(() => {
    if (!open) return;
    const timer = window.setInterval(() => void reread(), DIAGNOSTIC_REFRESH_MS);
    const onFocus = () => void reread();
    window.addEventListener('focus', onFocus);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener('focus', onFocus);
    };
  }, [open, reread]);

  const toggle = (path: string) =>
    setLeft((current) => {
      const next = new Set(current);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });

  const show = async (path: string) => {
    if (shown?.path === path) {
      setShown(null);
      return;
    }
    try {
      setShown({ path, text: await tauri.diagnosticShow(path) });
    } catch (e) {
      setMessage({ text: errorText(e), error: true });
    }
  };

  const save = async () => {
    setMessage(null);
    const target = await saveDialog({ defaultPath: fileName(), filters: [{ name: 'Zip file', extensions: ['zip'] }] });
    if (typeof target !== 'string') return;
    setBusy(true);
    try {
      const saved = await tauri.diagnosticSave(target, [...left]);
      setMessage({ text: `Saved to ${saved.path} (${size(saved.bytes)}). Nothing was sent anywhere.`, error: false });
      setPreview(null);
      onSaved?.();
    } catch (e) {
      setMessage({ text: `Not saved: ${errorText(e)}`, error: true });
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <SettingRow
        label="Save a diagnostic file"
        description="One .zip with your settings (keys removed), recent app messages and recordings of runs that failed. Nothing is sent anywhere."
      >
        <Btn disabled={busy} onClick={() => void review()}>
          {preview ? 'Refresh' : 'Review…'}
        </Btn>
      </SettingRow>
      {preview && (
        <SettingBlock
          label="What the file will contain"
          description="Passwords, keys and your user name are removed first. Untick anything you would rather leave out."
        >
          <ul className="st-diag-list" aria-label="Files in the diagnostic file">
            {preview.files.map((file) => (
              <li key={file.path} className="st-diag-item">
                <label className="st-diag-main">
                  <input type="checkbox" checked={!left.has(file.path)} onChange={() => toggle(file.path)} aria-label={file.label} />
                  <span className="st-diag-name">{file.label}</span>
                  <span className="st-diag-size">{size(file.bytes)}</span>
                </label>
                <Btn small onClick={() => void show(file.path)}>
                  {shown?.path === file.path ? 'Hide' : 'Show'}
                </Btn>
                {shown?.path === file.path && <pre className="st-diag-text">{shown.text}</pre>}
              </li>
            ))}
          </ul>
          <div className="st-diag-actions">
            <Btn primary disabled={busy || left.size >= preview.files.length} onClick={() => void save()}>
              Save…
            </Btn>
            <Btn onClick={() => setPreview(null)}>Cancel</Btn>
          </div>
        </SettingBlock>
      )}
      {message && <Note role={message.error ? 'alert' : 'status'}>{message.text}</Note>}
    </>
  );
}
