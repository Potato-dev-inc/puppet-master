// "Chat needs a small add-on for Claude (about 48 MB). Install": the card shown where Chat is chosen (New agent page)
// and where a chat start failed because the add-on is missing. Shows the install's progress and, if it failed, why.
import { progressText, promptText, type AddonStatus } from '../../lib/acpAddon';
import '../../styles/cockpit-acp.css';

export function AddonPrompt({ status, onInstall }: { status: AddonStatus; onInstall: () => void }) {
  const installing = status.state === 'installing';
  const failed = status.state === 'failed';
  return (
    <div className="ck-acp-card" data-testid="addon-prompt" role="group" aria-label={`Chat add-on for ${status.label}`}>
      <div className="ck-acp-card-body">
        <p className="ck-acp-card-title">{installing ? `Installing the chat add-on for ${status.label}…` : promptText(status)}</p>
        {installing && (
          <p className="ck-acp-card-line" role="status" data-testid="addon-progress">
            {progressText(status)}
          </p>
        )}
        {failed && (
          <p className="ck-acp-card-line is-error" role="alert" data-testid="addon-error">
            {status.detail || 'The install did not finish.'}
          </p>
        )}
      </div>
      {!installing && (
        <button type="button" className="ck-btn ck-btn--secondary" onClick={onInstall}>
          {failed ? 'Try again' : 'Install'}
        </button>
      )}
    </div>
  );
}
