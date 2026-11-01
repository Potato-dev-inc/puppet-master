// The two little lists that open above the message box: `@` (other bots) and `/` (commands). Mouse and keyboard both
// pick; the keys are handled by the box, which tells this list which row is active.
export interface PickerItem {
  id: string;
  label: string;
  hint?: string;
}

export interface ComposerPickerProps {
  /** The list's id, so the box can point at it (`aria-controls`). */
  id: string;
  label: string;
  items: readonly PickerItem[];
  active: number;
  /** A line shown when there is nothing to pick yet. */
  empty?: string;
  onPick: (item: PickerItem) => void;
  onHover: (index: number) => void;
}

export function pickerOptionId(listId: string, index: number): string {
  return `${listId}-${index}`;
}

export function ComposerPicker({ id, label, items, active, empty, onPick, onHover }: ComposerPickerProps) {
  return (
    <div className="ck-potato-picker ck-fade-in">
      {items.length === 0 ? (
        <p className="ck-potato-picker-empty" role="status">
          {empty}
        </p>
      ) : (
        <ul id={id} role="listbox" aria-label={label} className="ck-potato-picker-list">
          {items.map((item, i) => (
            <li
              key={item.id}
              id={pickerOptionId(id, i)}
              role="option"
              aria-selected={i === active}
              className={i === active ? 'ck-potato-picker-row ck-potato-picker-row--active' : 'ck-potato-picker-row'}
              // The box keeps the keyboard focus: pressing a row must not take it.
              onMouseDown={(e) => e.preventDefault()}
              onMouseEnter={() => onHover(i)}
              onClick={() => onPick(item)}
            >
              <span className="ck-potato-picker-label">{item.label}</span>
              {item.hint && <span className="ck-potato-picker-hint">{item.hint}</span>}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
