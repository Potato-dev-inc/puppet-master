// The bot's own actions at the foot of the Advanced tab: "Export chat" (the chat as one Markdown file, saved through the
// same save dialog as the pane exports) and "Delete bot". Deleting asks the person to type the bot's name first; the
// engine checks the name again and moves the bot's files aside, never deletes them.
import { save as saveDialog } from '@tauri-apps/plugin-dialog';
import { useId, useState } from 'react';
import { chatPageFileName } from '../../../lib/chatExport';
import { plainError } from '../../../lib/potato';
import { ADVANCED_COPY, nameTypedRight, type BotSettingsTransport } from '../../../lib/potato-botsettings';
import { Sheet } from '../Sheet';
import { AdvancedSection } from './AdvancedSection';

export interface AdvancedBotProps {
  bot: { id: string; name: string };
  transport: BotSettingsTransport;
  onChanged?: () => void;
  onNotify?: (text: string) => void;
}

export function AdvancedBot({ bot, transport, onChanged, onNotify }: AdvancedBotProps) {
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  const [sheetOpen, setSheetOpen] = useState(false);
  const [typed, setTyped] = useState('');
  const [deleting, setDeleting] = useState(false);
  const titleId = useId();

  const exportChat = async () => {
    setExporting(true);
    setError(null);
    setStatus(null);
    try {
      const out = await transport.exportChat(bot.id);
      const picked = await saveDialog({ defaultPath: chatPageFileName(new Date(), 'markdown'), filters: [{ name: 'Markdown', extensions: ['md'] }] });
      if (typeof picked === 'string') {
        const path = await transport.saveExport(out.token, picked);
        setStatus(ADVANCED_COPY.exported(path));
      }
    } catch (e) {
      setError(plainError(e, 'Could not export the chat.'));
    } finally {
      setExporting(false);
    }
  };

  const remove = async () => {
    setDeleting(true);
    try {
      await transport.remove(bot.id, typed);
      setSheetOpen(false);
      onChanged?.();
      onNotify?.(`${bot.name} was deleted.`);
    } catch (e) {
      setError(plainError(e, 'Could not delete the bot.'));
    } finally {
      setDeleting(false);
    }
  };

  return (
    <AdvancedSection title={ADVANCED_COPY.bot}>
      <div className="ck-potato-adv-actions ck-potato-adv-stack">
        <button type="button" className="ck-btn ck-btn--secondary" disabled={exporting} onClick={() => void exportChat()}>
          {exporting ? ADVANCED_COPY.exporting : ADVANCED_COPY.exportChat}
        </button>
        <button type="button" className="ck-btn ck-btn--plain ck-potato-adv-danger" onClick={() => setSheetOpen(true)}>
          {ADVANCED_COPY.remove}
        </button>
      </div>
      {status && (
        <p className="ck-potato-adv-status" role="status">
          {status}
        </p>
      )}
      {error && !sheetOpen && (
        <p className="ck-potato-adv-error" role="alert">
          {error}
        </p>
      )}
      {sheetOpen && (
        <Sheet labelledBy={titleId} onClose={deleting ? undefined : () => setSheetOpen(false)}>
          <h2 id={titleId} className="ck-potato-adv-sheet-title">
            {ADVANCED_COPY.removeTitle(bot.name)}
          </h2>
          <p className="ck-sub">{ADVANCED_COPY.removeLine}</p>
          <label className="ck-potato-adv-field">
            <span className="ck-potato-adv-line">{ADVANCED_COPY.typeToConfirm(bot.name)}</span>
            <input className="ck-potato-adv-input" type="text" value={typed} onChange={(event) => setTyped(event.target.value)} autoComplete="off" />
          </label>
          {error && (
            <p className="ck-potato-adv-error" role="alert">
              {error}
            </p>
          )}
          <div className="ck-potato-adv-actions">
            <button type="button" className="ck-btn ck-btn--secondary" disabled={deleting} onClick={() => setSheetOpen(false)}>
              {ADVANCED_COPY.cancel}
            </button>
            <button type="button" className="ck-btn ck-btn--primary" disabled={!nameTypedRight(typed, bot.name) || deleting} onClick={() => void remove()}>
              {ADVANCED_COPY.deleteNow}
            </button>
          </div>
        </Sheet>
      )}
    </AdvancedSection>
  );
}
