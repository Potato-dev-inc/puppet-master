import type { ReactNode } from 'react';

/** `a \`b\` c` -> a <code>b</code> c. Backticks never reach the screen; the command is a code chip. */
export function inlineCode(text: string): ReactNode[] {
  return text.split('`').map((part, i) => (i % 2 === 1 && part ? <code key={i} className="ck-code">{part}</code> : part));
}
