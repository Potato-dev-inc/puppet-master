// The little toolbar beside a message: Copy, Reply, then whatever the row adds (Speak, Save as file). It shows when the message is hovered or has the keyboard focus (and
// always on a touch screen). Copy puts the message's text on the clipboard; Reply sends a quote to the composer.
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { THREAD_COPY } from '../../../lib/potato-thread';
import '../../../styles/cockpit-potato-thread.css';

/** How long the button says "Copied". */
const COPIED_MS = 1500;

export interface HoverActionsProps {
  text: string;
  onReply: () => void;
  /** More buttons after Reply, in order (Speak, then Save as file). */
  children?: ReactNode;
}

export function HoverActions({ text, onReply, children }: HoverActionsProps) {
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
      // No clipboard here, or it said no: the button keeps saying Copy.
      return;
    }
    setCopied(true);
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = setTimeout(() => setCopied(false), COPIED_MS);
  };

  return (
    <div className="ck-potato-actions" role="toolbar" aria-label={THREAD_COPY.actionsLabel}>
      <button type="button" className="ck-potato-action" data-copied={copied || undefined} aria-label={`${THREAD_COPY.copy} message`} onClick={() => void copy()}>
        {copied ? THREAD_COPY.copied : THREAD_COPY.copy}
      </button>
      <button type="button" className="ck-potato-action" aria-label={`${THREAD_COPY.reply} to message`} onClick={onReply}>
        {THREAD_COPY.reply}
      </button>
      {children}
    </div>
  );
}
