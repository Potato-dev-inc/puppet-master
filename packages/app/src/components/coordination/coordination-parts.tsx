// Small presentational pieces of the coordination panel: metric tile, selectable data list, id/time formatters.

export function shortId(id: string): string {
  if (id.length <= 10) return id;
  return `${id.slice(0, 4)}…${id.slice(-4)}`;
}

export function relativeTime(timestampMs: number): string {
  const delta = Date.now() - timestampMs;
  if (delta < 1000) return 'now';
  if (delta < 60_000) return `${Math.floor(delta / 1000)}s`;
  if (delta < 3_600_000) return `${Math.floor(delta / 60_000)}m`;
  return `${Math.floor(delta / 3_600_000)}h`;
}

export function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="pm-coordination-metric">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}


export function DataList({
  empty,
  items,
  selectedId,
  onSelect,
}: {
  empty: string;
  items: Array<{ id: string; title: string; meta: string }>;
  selectedId?: string;
  onSelect?: (id: string) => void;
}) {
  if (items.length === 0) {
    return <div className="pm-coordination-empty">{empty}</div>;
  }
  return (
    <ul className="pm-coordination-list">
      {items.map((item) => (
        <li key={item.id} className={selectedId === item.id ? 'selected' : undefined}>
          {onSelect ? (
            <button
              type="button"
              className="pm-coordination-row-button"
              onClick={() => onSelect(item.id)}
            >
              <RowContent item={item} />
            </button>
          ) : (
            <RowContent item={item} />
          )}
        </li>
      ))}
    </ul>
  );
}


function RowContent({ item }: { item: { id: string; title: string; meta: string } }) {
  return (
    <>
      <span className="pm-coordination-list-main">
        <strong>{item.title}</strong>
        <small>{item.meta}</small>
      </span>
      <code>{shortId(item.id)}</code>
    </>
  );
}
