// The agent's answer, drawn without the markdown marks: **bold**, `code` and "- " lists, nothing else.
// An answer is plain words for a person, so a star or a backtick must never show.
import type { ReactNode } from 'react';

/** `**bold**` and `code` inside one line. */
export function inline(text: string): ReactNode[] {
  return text.split(/(\*\*[^*\n]+\*\*|`[^`\n]+`)/g).map((part, i) => {
    if (part.length > 4 && part.startsWith('**') && part.endsWith('**')) return <strong key={i}>{inline(part.slice(2, -2))}</strong>;
    if (part.length > 2 && part.startsWith('`') && part.endsWith('`')) return <code key={i} className="fr-code">{part.slice(1, -1)}</code>;
    return part;
  });
}

export type Block = { kind: 'p'; text: string } | { kind: 'ul'; items: string[] };

/** Blank lines separate paragraphs; consecutive "- " or "* " lines are one list. */
export function blocks(text: string): Block[] {
  const out: Block[] = [];
  for (const raw of text.replace(/\r\n/g, '\n').split('\n')) {
    const line = raw.trim();
    const item = /^[-*]\s+(.*)$/.exec(line);
    const last = out[out.length - 1];
    if (!line) {
      out.push({ kind: 'p', text: '' });
    } else if (item) {
      if (last?.kind === 'ul') last.items.push(item[1]);
      else out.push({ kind: 'ul', items: [item[1]] });
    } else if (last?.kind === 'p' && last.text !== '') {
      last.text += ` ${line}`;
    } else if (last?.kind === 'p' && last.text === '') {
      last.text = line;
    } else {
      out.push({ kind: 'p', text: line });
    }
  }
  return out.filter((b) => b.kind === 'ul' || b.text !== '');
}

export function Answer({ text }: { text: string }) {
  return (
    <div className="fr-answer" data-testid="task-answer">
      {blocks(text).map((b, i) =>
        b.kind === 'ul' ? (
          <ul key={i}>
            {b.items.map((it, j) => (
              <li key={j}>{inline(it)}</li>
            ))}
          </ul>
        ) : (
          <p key={i}>{inline(b.text)}</p>
        ),
      )}
    </div>
  );
}
