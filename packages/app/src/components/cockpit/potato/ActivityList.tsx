// The Activity view of the bot list: recent things across the bots, newest first. Each line is the bot, what happened and
// the time. A click opens that bot's thread. The events come from lib/potato-list.ts (activityEvents), not from storage.
import { listTime, LIST_COPY, type ActivityEvent } from '../../../lib/potato-list';

export interface ActivityListProps {
  events: ActivityEvent[];
  now: number;
  onOpen: (botId: string) => void;
}

export function ActivityList({ events, now, onOpen }: ActivityListProps) {
  if (events.length === 0) return <p className="ck-sub ck-potato-empty">{LIST_COPY.noActivity}</p>;
  return (
    <ul className="ck-potato-activity" aria-label={LIST_COPY.activity}>
      {events.map((event, i) => (
        <li key={`${event.botId}-${i}`}>
          <button type="button" className="ck-potato-act" onClick={() => onOpen(event.botId)}>
            <span className="ck-potato-act-top">
              <span className="ck-potato-act-name">{event.botName}</span>
              {event.at !== null && <span className="ck-potato-time">{listTime(event.at, now)}</span>}
            </span>
            <span className="ck-potato-act-text">{event.text}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}
