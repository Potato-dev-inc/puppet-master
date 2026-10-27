// The chat over a terminal pane: reads the pane's session file while it is mounted (TerminalPane mounts it
// only when the person picked Chat). `dock` is what waits on the person, shown under the conversation.
import type { ReactNode } from 'react';
import { useSessionChat } from '../../hooks/useSessionChat';
import { SessionChatView } from './SessionChatView';

export function SessionChatPanel({ paneId, dock }: { paneId: string; dock?: ReactNode }) {
  const chat = useSessionChat(paneId, true);
  return (
    <SessionChatView
      state={chat.state}
      reason={chat.reason}
      waitingFor={chat.waitingFor}
      source={chat.source}
      turns={chat.turns}
      model={chat.model}
      cliVersion={chat.cliVersion}
      dock={dock}
    />
  );
}
