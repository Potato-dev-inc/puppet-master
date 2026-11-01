// Sends what the New agent page asks for: starts the agent the same way the "+" menu does (a terminal pane through
// `newAgent`) or as a chat session (`acp_chat_start`, the Labs way), then queues the person's first message in the
// pane's outbox, which types it once the agent is ready. Nothing here names an agent.
import { invoke } from '@tauri-apps/api/core';
import { blockOf, type ChatBlock } from '../../../lib/acpAddon';
import { sendPaneMessage } from '../../../lib/paneOutbox';
import type { StartRequest } from './model';

export interface StartDeps {
  /** The cockpit's `newAgent`: opens the pane and the starting view; resolves to the pane id, or null if it failed. */
  newAgent: (agentId: string, accountId?: string, isolated?: boolean, projectPath?: string) => Promise<string | null>;
  /** Starts a chat session in `folder`; resolves to its pane id. Rejects with one plain sentence. */
  startChat: (agentId: string, folder: string) => Promise<string>;
  /** Opens a pane in the main area. */
  select: (paneId: string) => void;
  /** Queues a message for the pane. */
  send: (paneId: string, text: string) => Promise<unknown>;
  notify: (text: string) => void;
}

/** A chat start that stopped for a reason the page answers with a card: the add-on is missing, or a sign-in is needed. */
export interface StartBlock {
  blocked: ChatBlock;
  agentId: string;
  message: string;
}

export const isStartBlock = (value: unknown): value is StartBlock =>
  !!value && typeof value === 'object' && ((value as StartBlock).blocked === 'addon' || (value as StartBlock).blocked === 'signin');

const errText = (error: unknown): string => (error instanceof Error ? error.message : typeof error === 'string' ? error : 'Something went wrong.');

/** The real chat start. */
export async function startChatSession(agentId: string, folder: string): Promise<string> {
  const started = await invoke<{ pane_id: string }>('acp_chat_start', { agentId, folder, explicit: true });
  return started.pane_id;
}

export const realStartDeps = (
  cockpit: Pick<StartDeps, 'newAgent' | 'select' | 'notify'>,
): StartDeps => ({ ...cockpit, startChat: startChatSession, send: sendPaneMessage });

/**
 * Runs one request. Resolves to the pane id, or null when nothing started (the person was told why). A chat that cannot
 * start for want of its add-on or a sign-in resolves to a `StartBlock` instead, and nothing is announced: the page shows
 * the card. Never throws.
 */
export async function runStart(req: StartRequest, deps: StartDeps): Promise<string | StartBlock | null> {
  let paneId: string | null = null;
  try {
    if (req.runAs === 'chat') {
      if (!req.folder) {
        deps.notify('Pick a folder for a chat session.');
        return null;
      }
      paneId = await deps.startChat(req.agentId, req.folder);
      deps.select(paneId);
    } else {
      paneId = await deps.newAgent(req.agentId, req.accountId, req.isolated, req.folder);
    }
  } catch (error) {
    const message = errText(error);
    const blocked = req.runAs === 'chat' ? blockOf(message) : null;
    if (blocked) return { blocked, agentId: req.agentId, message };
    deps.notify(message);
    return null;
  }
  if (!paneId) return null; // the starting view shows why
  try {
    await deps.send(paneId, req.prompt);
  } catch (error) {
    deps.notify(`The agent started, but your message was not sent: ${errText(error)}`);
  }
  return paneId;
}
