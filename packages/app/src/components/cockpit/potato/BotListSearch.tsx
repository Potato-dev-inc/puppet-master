// The search box at the top of the bot list. It filters the bots by name and last message (lib/potato-list.ts). Escape
// clears it; the × button clears it too. The page's Ctrl/Cmd+K focuses it through the list's ref.
import type { RefObject } from 'react';
import { LIST_COPY } from '../../../lib/potato-list';

export interface BotListSearchProps {
  value: string;
  onChange: (value: string) => void;
  inputRef: RefObject<HTMLInputElement>;
}

export function BotListSearch({ value, onChange, inputRef }: BotListSearchProps) {
  return (
    <div className="ck-potato-search">
      <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" aria-hidden="true">
        <circle cx="7" cy="7" r="4.5" />
        <path d="M10.5 10.5L14 14" />
      </svg>
      <input
        ref={inputRef}
        type="text"
        className="ck-potato-search-input"
        aria-label={LIST_COPY.search}
        placeholder={LIST_COPY.searchPlaceholder}
        value={value}
        autoComplete="off"
        spellCheck={false}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Escape' && value) {
            e.preventDefault();
            e.stopPropagation();
            onChange('');
          }
        }}
      />
      {value && (
        <button
          type="button"
          className="ck-potato-search-clear"
          aria-label={LIST_COPY.clearSearch}
          title={LIST_COPY.clearSearch}
          onClick={() => {
            onChange('');
            inputRef.current?.focus();
          }}
        >
          ×
        </button>
      )}
    </div>
  );
}
