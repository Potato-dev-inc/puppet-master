// One row of the Review Desk list: two lines, about 44 px. Line one is the risk glyph, the title and the agent.
// Line two is the riskiest reason (muted) and the file count. Text only: titles, reasons and file names are React
// text nodes, never markup.
import { levelGlyph, type ReviewItem } from '../../../lib/review-desk';
import { plural } from '../diff';

const number = (n: number): string => n.toLocaleString('en-US');

/** "3 files +52 −20", or "" when the item has no files. */
export function filesLine(item: ReviewItem): string {
  const { total, added, deleted } = item.files;
  if (total === 0) return '';
  return `${plural(total, 'file')} +${number(added)} −${number(deleted)}`;
}

/** The riskiest reason, and how many more follow. "No flags" when there are none. */
export function reasonLine(item: ReviewItem): string {
  const first = item.reasons[0];
  if (!first) return 'No flags';
  const more = item.reasons.length - 1;
  return more > 0 ? `${first.sentence} and ${more} more` : first.sentence;
}

export function ReviewRow({
  item,
  selected,
  grouped,
  onFocus,
  onOpen,
}: {
  item: ReviewItem;
  selected: boolean;
  /** The row sits under a group header, so it is indented. */
  grouped: boolean;
  onFocus: () => void;
  onOpen: () => void;
}) {
  return (
    <li
      className={`ck-rd-row${selected ? ' is-selected' : ''}${grouped ? ' is-grouped' : ''}`}
      data-rd="item"
      data-rd-row={item.key}
      data-level={item.level}
      tabIndex={-1}
      aria-current={selected ? 'true' : undefined}
      onFocus={onFocus}
      onClick={onOpen}
    >
      <div className="ck-rd-line">
        <span className="ck-rd-glyph" role="img" aria-label={item.levelWord} title={item.levelWord}>
          {levelGlyph(item.level)}
        </span>
        <span className="ck-rd-title" title={item.title}>
          {item.title}
        </span>
        <span className="ck-rd-agent">{item.agentLabel}</span>
      </div>
      <div className="ck-rd-sub">
        <span className="ck-rd-reason">{reasonLine(item)}</span>
        <span className="ck-rd-files">{filesLine(item)}</span>
      </div>
    </li>
  );
}
