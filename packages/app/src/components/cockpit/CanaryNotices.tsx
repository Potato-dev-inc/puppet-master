// Inbox info cards from the version check: the result of an update, good or not ("Codex 0.159.3 ->
// 0.162.1: checked, all good." / "...: screens changed. Puppet Master will ask you before acting.").
// They block nothing, so they do not count toward "things need you". They use the same card as
// everything else, with the calm glyph and no number.
import { tauriCanaryApi, useCanary, type CanaryApi } from '../../lib/canary';
import '../../styles/canary.css';
import { CardButton, InboxCard } from './InboxCard';

export function CanaryNotices({ api = tauriCanaryApi }: { api?: CanaryApi }) {
  const { status, refresh } = useCanary(api);
  if (status.notices.length === 0) return null;
  return (
    <ul className="ib-list" aria-label="Notes about your agents">
      {status.notices.map((n) => (
        <li key={n.id}>
          <InboxCard
            kind="note"
            glyph="notice"
            info
            title={n.text}
            data={{ 'data-testid': 'canary-notice', 'data-result': n.kind }}
            more={
              <CardButton tone="plain" role="dismiss" onClick={() => void api.dismissNotice(n.id).then(refresh)}>
                Dismiss
              </CardButton>
            }
          />
        </li>
      ))}
    </ul>
  );
}
