// What a chat says while its conversation is open in a terminal pane: the chat is paused (its message box is switched off
// in PaneView), with the way back and a link to the terminal.
import '../../styles/cockpit-acp.css';

export const HANDED_NOTE = 'This conversation is open in a terminal';
export const HANDED_BLOCK = 'This conversation is open in a terminal. Go back to chat to write here.';

export function HandedToTerminal({ terminalPaneId, busy, onBack, onGoToTerminal }: { terminalPaneId: string | null; busy: boolean; onBack: () => void; onGoToTerminal: (paneId: string) => void }) {
  return (
    <div className="ck-acp-strip ck-acp-handed" data-testid="acp-handed">
      <span className="ck-acp-strip-text" role="status">
        {HANDED_NOTE}
      </span>
      <span className="ck-acp-handed-actions">
        {terminalPaneId && (
          <button type="button" className="ck-acp-link" onClick={() => onGoToTerminal(terminalPaneId)}>
            Go to the terminal
          </button>
        )}
        <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={onBack}>
          Back to chat
        </button>
      </span>
    </div>
  );
}
