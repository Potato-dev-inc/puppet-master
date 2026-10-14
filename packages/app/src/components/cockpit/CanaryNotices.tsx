// Inbox info cards from the version check ("Codex 0.162 changed its screens. Puppet Master will ask
// you before acting."). They block nothing, so they do not count toward "things need you".
import { tauriCanaryApi, useCanary, type CanaryApi } from '../../lib/canary';
import '../../styles/canary.css';

export function CanaryNotices({ api = tauriCanaryApi }: { api?: CanaryApi }) {
  const { status, refresh } = useCanary(api);
  if (status.notices.length === 0) return null;
  return (
    <ul className="ck-canary-list" aria-label="Notes about your agents">
      {status.notices.map((n) => (
        <li key={n.id} className="ck-canary-card" data-testid="canary-notice">
          <p className="ck-canary-text">{n.text}</p>
          <div className="ck-canary-actions">
            <button
              type="button"
              className="ck-btn ck-btn--sm ck-btn--secondary"
              onClick={() => void api.dismissNotice(n.id).then(refresh)}
            >
              Dismiss
            </button>
          </div>
        </li>
      ))}
    </ul>
  );
}
