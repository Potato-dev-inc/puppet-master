// The agent's own text (a session file) as formatted text: headings, bold, code, lists, quotes, rules and tables.
// The tree comes from lib/chatMarkdown; every piece is a React element with text children, so no HTML from the
// message is ever created. Links show their text and keep the address in `title`; they are not clickable.
import type { ReactNode } from 'react';
import { parseChatMarkdown, type Align, type Block, type Inline, type ListBlock } from '../../lib/chatMarkdown';

export interface ChatMarkdownProps {
  text: string;
}

export function ChatMarkdown({ text }: ChatMarkdownProps) {
  return <div className="ck-chat-md min-w-0 break-words ck-chat-text">{renderBlocks(safeParse(text))}</div>;
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
      return (
        <pre key={key} data-lang={block.lang || undefined} className="ck-chat-md-pre rounded border border-pm-border bg-pm-panel">
          <code>{block.text}</code>
        </pre>
      );
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

function renderList(list: ListBlock, key?: number): ReactNode {
  const items = list.items.map((item, k) => (
    <li key={k}>
      {renderInlines(item.children)}
      {item.sub && renderList(item.sub)}
    </li>
  ));
  if (list.ordered) {
    return (
      <ol key={key} start={list.start !== 1 ? list.start : undefined} className="ck-chat-md-list ck-chat-md-ol">
        {items}
      </ol>
    );
  }
  return <ul key={key} className="ck-chat-md-list ck-chat-md-ul">{items}</ul>;
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
        <span key={key} className="ck-chat-md-link" title={node.url}>
          {renderInlines(node.children)}
        </span>
      );
  }
}
