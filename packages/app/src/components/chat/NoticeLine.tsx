// A notice of the session (context compacted, an error, a pull request opened) as a thin centred line between two
// rules. Plain text only: a link's address is the tooltip, not a link.

export interface NoticeLineProps {
  text: string;
  /** `error` is shown heavier; other kinds are the same line. */
  kind: string;
  url?: string | null;
}

export function NoticeLine({ text, kind, url }: NoticeLineProps) {
  const isError = kind === 'error';
  return (
    <div className="flex items-center gap-2 py-0.5 text-[11px] text-pm-muted ck-chat-notice" data-notice={kind}>
      <span aria-hidden="true" className="h-px flex-1 bg-pm-border ck-chat-rule" />
      <span title={url ?? undefined} className={`min-w-0 text-center break-words ck-chat-notice-text${isError ? ' ck-chat-notice-text--error' : ''}`}>
        {text}
      </span>
      <span aria-hidden="true" className="h-px flex-1 bg-pm-border ck-chat-rule" />
    </div>
  );
}
