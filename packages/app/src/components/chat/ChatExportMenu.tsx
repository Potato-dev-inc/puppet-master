// The chat's header menu. "Save as page" and "Save as Markdown" show the chat in a preview and save that same text
// where the person picks. The sheet has a switch between Page and Markdown. Nothing is uploaded. The page preview is
// an iframe with an empty sandbox (no scripts) and the page as srcdoc, so the page cannot run code or reach anything.
// The Markdown preview is plain text.
import { useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { save as saveDialog } from '@tauri-apps/plugin-dialog';
import { chatExportHtml, chatExportMarkdown, chatExportSave, chatPageFileName, type ExportFormat } from '../../lib/chatExport';
import { IconMore } from '../cockpit/Icons';
import { SegTabs } from '../cockpit/SegTabs';
import { Sheet } from '../cockpit/Sheet';

/** What the menu needs from the desktop shell (a fake in tests). */
export interface ChatExportApi {
  html: (paneId: string) => Promise<{ html: string; token: string }>;
  markdown: (paneId: string) => Promise<{ markdown: string; token: string }>;
  save: (token: string, path: string) => Promise<string>;
  /** The path the person picked, or null when they cancelled. */
  pickPath: (defaultName: string, format: ExportFormat) => Promise<string | null>;
}

const FILTERS: Record<ExportFormat, { name: string; extensions: string[] }> = {
  page: { name: 'Web page', extensions: ['html'] },
  markdown: { name: 'Markdown', extensions: ['md'] },
};

export const tauriChatExportApi: ChatExportApi = {
  html: chatExportHtml,
  markdown: chatExportMarkdown,
  save: chatExportSave,
  pickPath: async (defaultName, format) => {
    const picked = await saveDialog({ defaultPath: defaultName, filters: [FILTERS[format]] });
    return typeof picked === 'string' ? picked : null;
  },
};

const MENU_ITEMS: { format: ExportFormat; label: string }[] = [
  { format: 'page', label: 'Save as page' },
  { format: 'markdown', label: 'Save as Markdown' },
];

const FORMAT_TABS = [
  { id: 'page' as const, label: 'Page' },
  { id: 'markdown' as const, label: 'Markdown' },
];

/** A text Rust made: the preview and the save both use `text`, and only `token` saves it. */
interface Made {
  format: ExportFormat;
  text: string;
  token: string;
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function ChatExportMenu({ paneId, api = tauriChatExportApi }: { paneId: string; api?: ChatExportApi }) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [preview, setPreview] = useState<ExportFormat | null>(null);
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
            {MENU_ITEMS.map((item) => (
              <button
                key={item.format}
                type="button"
                className="ck-menu-item"
                role="menuitem"
                onClick={() => {
                  setMenuOpen(false);
                  setPreview(item.format);
                }}
              >
                {item.label}
              </button>
            ))}
          </div>
        )}
      </div>
      {preview !== null && <SavePagePreview paneId={paneId} api={api} initial={preview} onClose={() => setPreview(null)} />}
    </>
  );
}

/** The chat as the file will hold it (a page, or Markdown), and the save. The preview and the saved file are the same text. */
export function SavePagePreview({
  paneId,
  api,
  initial = 'page',
  onClose,
}: {
  paneId: string;
  api: ChatExportApi;
  initial?: ExportFormat;
  onClose: () => void;
}) {
  const titleId = useId();
  const [format, setFormat] = useState<ExportFormat>(initial);
  const [made, setMade] = useState<Made | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  // Bumped after each save: a token works once, so the text is made again for the next save.
  const [round, setRound] = useState(0);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);

  useEffect(() => {
    let alive = true;
    setLoadError(null);
    const asked: Promise<Made> =
      format === 'markdown'
        ? api.markdown(paneId).then((m) => ({ format, text: m.markdown, token: m.token }))
        : api.html(paneId).then((p) => ({ format, text: p.html, token: p.token }));
    asked
      .then((next) => {
        if (alive) setMade(next);
      })
      .catch((error: unknown) => {
        if (!alive) return;
        setMade(null);
        setLoadError(errorText(error));
      });
    return () => {
      alive = false;
    };
  }, [api, paneId, format, round]);

  // Only the text of the format on the switch is shown or saved.
  const current = made?.format === format ? made : null;
  const noun = format === 'markdown' ? 'file' : 'page';

  const switchTo = (next: ExportFormat) => {
    if (next === format) return;
    setFormat(next);
    setMessage(null);
  };

  const save = async () => {
    if (current === null) return;
    setMessage(null);
    let path: string | null;
    try {
      path = await api.pickPath(chatPageFileName(new Date(), format), format);
    } catch (error) {
      setMessage({ text: `Not saved: ${errorText(error)}`, error: true });
      return;
    }
    if (!path) return;
    setBusy(true);
    try {
      const saved = await api.save(current.token, path);
      setMessage({ text: `Saved to ${saved}. Nothing was sent anywhere.`, error: false });
      setMade(null);
      setRound((n) => n + 1);
    } catch (error) {
      setMessage({ text: `Not saved: ${errorText(error)}`, error: true });
    } finally {
      setBusy(false);
    }
  };

  const body: ReactNode =
    current === null ? (
      <p className="ck-sub">{loadError ?? `Making the ${noun}…`}</p>
    ) : format === 'markdown' ? (
      <pre className="ck-chat-export-text">{current.text}</pre>
    ) : (
      <iframe className="ck-chat-export-frame" title="Chat page preview" sandbox="" srcDoc={current.text} />
    );

  return (
    <Sheet labelledBy={titleId} role="dialog" onClose={busy ? undefined : onClose} className="ck-chat-export">
      <h2 id={titleId} className="ck-section">
        {format === 'markdown' ? 'Save as Markdown' : 'Save as page'}
      </h2>
      <p className="ck-sub">
        {format === 'markdown'
          ? 'One Markdown file with this chat. Nothing is uploaded. Any text editor or Markdown viewer opens it.'
          : 'One HTML file with this chat. Nothing is uploaded. It opens in any browser and can be sent as it is.'}
      </p>
      <div className="ck-chat-export-switch">
        <SegTabs label="File format" options={FORMAT_TABS} value={format} onChange={switchTo} />
      </div>
      <div className="ck-chat-export-body">{body}</div>
      {message && (
        <p role={message.error ? 'alert' : 'status'} className="ck-sub">
          {message.text}
        </p>
      )}
      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn ck-btn--primary" disabled={current === null || busy} onClick={() => void save()}>
          {format === 'markdown' ? 'Save as Markdown…' : 'Save as page…'}
        </button>
        <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={onClose}>
          Close
        </button>
      </div>
    </Sheet>
  );
}
