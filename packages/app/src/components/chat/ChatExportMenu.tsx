// The chat's header menu. "Save as page" shows the chat as one HTML page in a preview and saves that same page where
// the person picks. Nothing is uploaded. The preview is an iframe with an empty sandbox (no scripts) and the page as
// srcdoc, so the page cannot run code or reach anything.
import { useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { save as saveDialog } from '@tauri-apps/plugin-dialog';
import { chatExportHtml, chatExportSave, chatPageFileName, type ExportPage } from '../../lib/chatExport';
import { IconMore } from '../cockpit/Icons';
import { Sheet } from '../cockpit/Sheet';

/** What the menu needs from the desktop shell (a fake in tests). */
export interface ChatExportApi {
  html: (paneId: string) => Promise<ExportPage>;
  save: (token: string, path: string) => Promise<string>;
  /** The path the person picked, or null when they cancelled. */
  pickPath: (defaultName: string) => Promise<string | null>;
}

export const tauriChatExportApi: ChatExportApi = {
  html: chatExportHtml,
  save: chatExportSave,
  pickPath: async (defaultName) => {
    const picked = await saveDialog({ defaultPath: defaultName, filters: [{ name: 'Web page', extensions: ['html'] }] });
    return typeof picked === 'string' ? picked : null;
  },
};

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function ChatExportMenu({ paneId, api = tauriChatExportApi }: { paneId: string; api?: ChatExportApi }) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [previewOpen, setPreviewOpen] = useState(false);
  const wrap = useRef<HTMLDivElement>(null);

  // A press outside the menu, or Escape, closes it.
  useEffect(() => {
    if (!menuOpen) return;
    const onPress = (event: MouseEvent) => {
      if (!wrap.current?.contains(event.target as Node)) setMenuOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setMenuOpen(false);
    };
    document.addEventListener('mousedown', onPress);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onPress);
      document.removeEventListener('keydown', onKey);
    };
  }, [menuOpen]);

  return (
    <>
      <div className="ck-menu-wrap" ref={wrap}>
        <button type="button" className="ck-icon-btn" aria-label="Chat menu" aria-haspopup="menu" aria-expanded={menuOpen} onClick={() => setMenuOpen((open) => !open)}>
          <IconMore />
        </button>
        {menuOpen && (
          <div className="ck-menu" role="menu">
            <button
              type="button"
              className="ck-menu-item"
              role="menuitem"
              onClick={() => {
                setMenuOpen(false);
                setPreviewOpen(true);
              }}
            >
              Save as page
            </button>
          </div>
        )}
      </div>
      {previewOpen && <SavePagePreview paneId={paneId} api={api} onClose={() => setPreviewOpen(false)} />}
    </>
  );
}

/** The page as the file will hold it, and the save. The preview and the saved file are the same text. */
export function SavePagePreview({ paneId, api, onClose }: { paneId: string; api: ChatExportApi; onClose: () => void }) {
  const titleId = useId();
  const [page, setPage] = useState<ExportPage | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .html(paneId)
      .then((made) => alive && setPage(made))
      .catch((error: unknown) => alive && setLoadError(errorText(error)));
    return () => {
      alive = false;
    };
  }, [api, paneId]);

  const save = async () => {
    if (page === null) return;
    setMessage(null);
    let path: string | null;
    try {
      path = await api.pickPath(chatPageFileName());
    } catch (error) {
      setMessage({ text: `Not saved: ${errorText(error)}`, error: true });
      return;
    }
    if (!path) return;
    setBusy(true);
    try {
      const saved = await api.save(page.token, path);
      setMessage({ text: `Saved to ${saved}. Nothing was sent anywhere.`, error: false });
      // A token works once. Make the page again so "Save as page…" can save another copy; the preview shows the new page.
      api
        .html(paneId)
        .then(setPage)
        .catch(() => setPage(null));
    } catch (error) {
      setMessage({ text: `Not saved: ${errorText(error)}`, error: true });
    } finally {
      setBusy(false);
    }
  };

  const body: ReactNode =
    page !== null ? <iframe className="ck-chat-export-frame" title="Chat page preview" sandbox="" srcDoc={page.html} /> : <p className="ck-sub">{loadError ?? 'Making the page…'}</p>;

  return (
    <Sheet labelledBy={titleId} role="dialog" onClose={busy ? undefined : onClose} className="ck-chat-export">
      <h2 id={titleId} className="ck-section">
        Save as page
      </h2>
      <p className="ck-sub">One HTML file with this chat. Nothing is uploaded. It opens in any browser and can be sent as it is.</p>
      <div className="ck-chat-export-body">{body}</div>
      {message && (
        <p role={message.error ? 'alert' : 'status'} className="ck-sub">
          {message.text}
        </p>
      )}
      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn ck-btn--primary" disabled={page === null || busy} onClick={() => void save()}>
          Save as page…
        </button>
        <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={onClose}>
          Close
        </button>
      </div>
    </Sheet>
  );
}
