// The filter chips under the "Search all chats" box: Agent, Folder, When and Who. Each one opens a small menu of
// choices ("Any" first). The menu lists what the index holds (agents and folders), so nothing here is hard-coded.
import { useState } from 'react';
import { WHEN_LABELS, WHO_LABELS, type ChatFacetNames, type ChatFilters, type ChatWhen, type ChatWho } from '../../lib/chatFilters';

export type ChipKey = 'agent' | 'folder' | 'when' | 'who';

interface Choice {
  value: string | undefined;
  label: string;
}

const ANY = 'Any';

function Chip({
  label,
  picked,
  choices,
  value,
  open,
  onToggle,
  onPick,
}: {
  /** The chip's name, as it reads with nothing picked ("Agent"). */
  label: string;
  /** The text of the picked choice, when one is picked. */
  picked?: string;
  choices: Choice[];
  value: string | undefined;
  open: boolean;
  onToggle: () => void;
  onPick: (value: string | undefined) => void;
}) {
  return (
    <div className="ck-chat-chip-wrap">
      <button type="button" className={`ck-chat-chip${picked ? ' is-on' : ''}`} aria-haspopup="listbox" aria-expanded={open} onClick={onToggle}>
        {picked ? `${label}: ${picked}` : label} ▾
      </button>
      {open && (
        <ul className="ck-chat-chip-menu" role="listbox" aria-label={label}>
          {choices.map((choice) => (
            <li key={choice.value ?? ANY} role="none">
              <button
                type="button"
                role="option"
                aria-selected={choice.value === value}
                className={`ck-chat-chip-choice${choice.value === value ? ' is-picked' : ''}`}
                onClick={() => onPick(choice.value)}
              >
                {choice.label}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export function ChatFilterChips({
  filters,
  facets,
  agentName,
  onPick,
}: {
  /** The filters in force: the chips with the typed shortcuts on top. */
  filters: ChatFilters;
  facets: ChatFacetNames;
  /** The name the person knows an agent by (its id is the value). */
  agentName: (agentId: string) => string;
  /** A choice was made for one chip (`undefined`: Any). */
  onPick: (key: ChipKey, value: string | undefined) => void;
}) {
  const [open, setOpen] = useState<ChipKey | null>(null);
  const toggle = (key: ChipKey) => setOpen((now) => (now === key ? null : key));
  const pick = (key: ChipKey, value: string | undefined) => {
    setOpen(null);
    onPick(key, value);
  };

  const anyOf = (label: string): Choice => ({ value: undefined, label: `${ANY} ${label}` });
  const agentChoices: Choice[] = [anyOf('agent'), ...facets.agents.map((id) => ({ value: id, label: agentName(id) }))];
  const folderChoices: Choice[] = [anyOf('folder'), ...facets.folders.map((name) => ({ value: name, label: name }))];
  const whenChoices: Choice[] = [{ value: undefined, label: ANY }, ...(Object.keys(WHEN_LABELS) as ChatWhen[]).map((when) => ({ value: when, label: WHEN_LABELS[when] }))];
  const whoChoices: Choice[] = [{ value: undefined, label: 'Anyone' }, ...(Object.keys(WHO_LABELS) as ChatWho[]).map((who) => ({ value: who, label: WHO_LABELS[who] }))];

  return (
    <div className="ck-chat-chips" role="group" aria-label="Filter chats">
      <Chip
        label="Agent"
        picked={filters.agent ? agentName(filters.agent) : undefined}
        choices={agentChoices}
        value={filters.agent}
        open={open === 'agent'}
        onToggle={() => toggle('agent')}
        onPick={(value) => pick('agent', value)}
      />
      <Chip
        label="Folder"
        picked={filters.folder}
        choices={folderChoices}
        value={filters.folder}
        open={open === 'folder'}
        onToggle={() => toggle('folder')}
        onPick={(value) => pick('folder', value)}
      />
      <Chip
        label="When"
        picked={filters.when ? WHEN_LABELS[filters.when] : undefined}
        choices={whenChoices}
        value={filters.when}
        open={open === 'when'}
        onToggle={() => toggle('when')}
        onPick={(value) => pick('when', value)}
      />
      <Chip
        label="Who"
        picked={filters.who ? WHO_LABELS[filters.who] : undefined}
        choices={whoChoices}
        value={filters.who}
        open={open === 'who'}
        onToggle={() => toggle('who')}
        onPick={(value) => pick('who', value)}
      />
    </div>
  );
}
