// The agent's own text (a session file) as formatted text: headings, bold, code, lists, quotes, rules and tables.
// The tree comes from lib/chatMarkdown; every piece is a React element with text children, so no HTML from the
// message is ever created. Links show their text and keep the address in `title`; they are not clickable.
import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from 'react';
import { parseChatMarkdown, type Align, type Block, type Inline, type ListBlock, type ListItem } from '../../lib/chatMarkdown';
import '../../styles/chat-copy.css';

export interface ChatMarkdownProps {
  text: string;
  /** Given, an http(s) link is a real link that hands its address here (the app's external-link helper) instead of
   *  navigating. Absent, links stay inert text with the address in `title`. Other schemes are never links. */
  onOpenLink?: (url: string) => void;
}

const OpenLink = createContext<((url: string) => void) | undefined>(undefined);

const WEB_URL = /^https?:\/\/[^\s<>"']+$/i;

/** How long the button says "Copied". */
const COPIED_MS = 1500;

export function ChatMarkdown({ text, onOpenLink }: ChatMarkdownProps) {
  return (
    <OpenLink.Provider value={onOpenLink}>
      <div className="ck-chat-md min-w-0 break-words ck-chat-text">{renderBlocks(safeParse(text))}</div>
    </OpenLink.Provider>
  );
}

/** A link: inert text, or (when the page gave an opener and the address is http or https) a real anchor that hands the
 *  address to the opener. The anchor never navigates on its own. */
function LinkNode({ url, children }: { url: string; children: ReactNode }) {
  const open = useContext(OpenLink);
  if (!open || !WEB_URL.test(url)) {
    return (
      <span className="ck-chat-md-link" title={url}>
        {children}
      </span>
    );
  }
  return (
    <a
      className="ck-chat-md-link ck-chat-md-link--live"
      href={url}
      title={url}
      target="_blank"
      rel="noopener noreferrer"
      onClick={(e) => {
        e.preventDefault();
        open(url);
      }}
    >
      {children}
    </a>
  );
}

function safeParse(text: string): Block[] {
  try {
    return parseChatMarkdown(text);
  } catch {
    // The parser does not throw; if it ever did, the text still shows, as it was written.
    return [{ kind: 'paragraph', children: [{ kind: 'text', text }] }];
  }
}

function renderBlocks(blocks: Block[]): ReactNode[] {
  return blocks.map((block, k) => renderBlock(block, k));
}

function renderBlock(block: Block, key: number): ReactNode {
  switch (block.kind) {
    case 'paragraph':
      return <p key={key} className="ck-chat-md-p">{renderInlines(block.children)}</p>;
    case 'heading': {
      const children = renderInlines(block.children);
      const cls = `ck-chat-md-h ck-chat-md-h${block.level}`;
      if (block.level === 1) return <h1 key={key} className={cls}>{children}</h1>;
      if (block.level === 2) return <h2 key={key} className={cls}>{children}</h2>;
      return <h3 key={key} className={cls}>{children}</h3>;
    }
    case 'code':
      return <CodeBlock key={key} lang={block.lang} text={block.text} />;
    case 'list':
      return renderList(block, key);
    case 'quote':
      return <blockquote key={key} className="ck-chat-md-quote">{renderBlocks(block.children)}</blockquote>;
    case 'rule':
      return <hr key={key} className="ck-chat-md-rule" />;
    case 'table':
      return renderTable(block, key);
  }
}

/** A fenced block with a Copy button. The button copies the code as written: no language tag, no added newline. */
function CodeBlock({ lang, text }: { lang: string; text: string }) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (timer.current !== null) clearTimeout(timer.current);
    },
    [],
  );

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // No clipboard here (or it said no): the button keeps saying Copy.
      return;
    }
    setCopied(true);
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = setTimeout(() => setCopied(false), COPIED_MS);
  };

  return (
    <div className="ck-chat-md-code-wrap">
      <pre data-lang={lang || undefined} className="ck-chat-md-pre rounded border border-pm-border bg-pm-panel">
        <code>{text}</code>
      </pre>
      <button type="button" className="ck-chat-md-copy" data-copied={copied || undefined} onClick={() => void copy()}>
        {copied ? 'Copied' : 'Copy'}
      </button>
    </div>
  );
}

/** A done-mark at the start of an item (a tick, then the words): the same look as a ticked task. */
const DONE_MARK = /^[\u2713\u2714]\uFE0F?\s+/;

/** What an item shows: ticked or not (a task item, or text that starts with a tick), and the inlines after the mark. */
function itemMark(item: ListItem): { checked: boolean | null; children: Inline[] } {
  if (item.checked !== undefined) return { checked: item.checked, children: item.children };
  const first = item.children[0];
  if (first && first.kind === 'text' && DONE_MARK.test(first.text)) {
    return { checked: true, children: [{ kind: 'text', text: first.text.replace(DONE_MARK, '') }, ...item.children.slice(1)] };
  }
  return { checked: null, children: item.children };
}

function renderList(list: ListBlock, key?: number): ReactNode {
  const marks = list.items.map(itemMark);
  const checklist = !list.ordered && marks.some((m) => m.checked !== null);
  const items = list.items.map((item, k) => {
    const { checked, children } = marks[k];
    if (!checklist) {
      return (
        <li key={k}>
          {renderInlines(children)}
          {item.sub && renderList(item.sub)}
        </li>
      );
    }
    return (
      <li key={k} className="ck-chat-md-task" data-checked={checked === null ? undefined : String(checked)}>
        <span className="ck-chat-md-box" role="img" aria-label={checked ? 'Done' : 'Not done'} />
        <span className="ck-chat-md-task-body">
          {renderInlines(children)}
          {item.sub && renderList(item.sub)}
        </span>
      </li>
    );
  });
  if (list.ordered) {
    return (
      <ol key={key} start={list.start !== 1 ? list.start : undefined} className="ck-chat-md-list ck-chat-md-ol">
        {items}
      </ol>
    );
  }
  return <ul key={key} className={`ck-chat-md-list ck-chat-md-ul${checklist ? ' ck-chat-md-checklist' : ''}`}>{items}</ul>;
}

function alignClass(align: Align): string | undefined {
  return align ? `ck-chat-md-align-${align}` : undefined;
}

function renderTable(table: Extract<Block, { kind: 'table' }>, key: number): ReactNode {
  return (
    <div key={key} className="ck-chat-md-table-wrap">
      <table className="ck-chat-md-table">
        <thead>
          <tr>
            {table.header.map((cell, c) => (
              <th key={c} className={alignClass(table.align[c] ?? null)}>
                {renderInlines(cell)}
              </th>
            ))}
          </tr>
        </thead>
        {table.rows.length > 0 && (
          <tbody>
            {table.rows.map((row, r) => (
              <tr key={r}>
                {row.map((cell, c) => (
                  <td key={c} className={alignClass(table.align[c] ?? null)}>
                    {renderInlines(cell)}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        )}
      </table>
    </div>
  );
}

function renderInlines(nodes: Inline[]): ReactNode[] {
  return nodes.map((node, k) => renderInline(node, k));
}

function renderInline(node: Inline, key: number): ReactNode {
  switch (node.kind) {
    case 'text':
      return node.text;
    case 'break':
      return <br key={key} />;
    case 'code':
      return (
        <code key={key} className="ck-chat-md-code rounded border border-pm-border bg-pm-panel">
          {node.text}
        </code>
      );
    case 'strong':
      return <strong key={key}>{renderInlines(node.children)}</strong>;
    case 'em':
      return <em key={key}>{renderInlines(node.children)}</em>;
    case 'link':
      return (
        <LinkNode key={key} url={node.url}>
          {renderInlines(node.children)}
        </LinkNode>
      );
  }
}
