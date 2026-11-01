// The quiet lines between messages: a day ("Today"), a time ("12:11 AM") after a pause, and the "New" line above the
// first unread message. Grayscale only; none of them takes a click.
import { THREAD_COPY } from '../../../lib/potato-thread';
import '../../../styles/cockpit-potato-thread.css';

export function DaySeparator({ label }: { label: string }) {
  return (
    <li className="ck-potato-sep ck-potato-sep--day" role="separator" aria-label={label}>
      <span>{label}</span>
    </li>
  );
}

export function TimeSeparator({ label }: { label: string }) {
  return (
    <li className="ck-potato-sep ck-potato-sep--time" role="separator" aria-label={label}>
      <span>{label}</span>
    </li>
  );
}

export function UnreadDivider() {
  return (
    <li className="ck-potato-sep ck-potato-sep--unread" role="separator" aria-label={THREAD_COPY.newMessages}>
      <span>{THREAD_COPY.newMessages}</span>
    </li>
  );
}
