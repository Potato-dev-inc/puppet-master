import type { ChatMessage, LlmResponse } from './llm';
import { streamLlm } from './llm';
import { executeMcpTool, formatPaneList, PUPPET_MASTER_TOOLS, type McpToolExecutor } from './mcp-tools';
import type { LlmModel } from '@puppet-master/shared';

export interface PuppetMasterCallbacks {
  onAssistantText: (text: string) => void;
  onToolCall: (tool: string, args: unknown, result?: string, error?: string) => void;
  onComplete: () => void;
  onError: (err: Error) => void;
}

const SYSTEM_PROMPT = `You are the Puppet Master: an orchestrator that drives multiple coding-agent terminals inside the user's GUI.

You have these tools — list_panes, list_agent_contexts, read_agent_context, inspect_agent_model, spawn_agent, read_terminal_buffer, write_terminal_input, kill_pane_process.

IMPORTANT — reuse existing panes:
- ALWAYS call list_panes first.
- Panes with id puppet-master-orchestrator-* (role=orchestrator) are the dedicated orchestrator terminals — NEVER write_terminal_input, kill, or spawn_agent into them. Delegate only to worker panes.
- Call list_agent_contexts or inspect_agent_model before splitting work across multiple agents, then route harder tasks to stronger coding agents and deterministic shell work to shell panes.
- The user may already have agent terminals open (created via New session). NEVER spawn_agent if a worker pane of that agent_type already exists unless the user explicitly asks for another pane.
- spawn_agent automatically reuses an existing worker pane of the same agent_type. Use force_new only when the user wants a second pane of the same agent.

Critical workflow for agent TUIs (claude, codex, opencode):
1. list_panes — check what is already open
2. spawn_agent only if no matching pane exists (or reuse is returned)
3. write_terminal_input with append_newline=true to submit the user's task (REQUIRED — this presses Enter)
4. read_terminal_buffer once to confirm input was received
5. Do NOT read the buffer repeatedly without writing

write_terminal_input: ALWAYS use append_newline=true when sending a prompt to an agent.

Permission prompts (Allow once / Deny) are auto-approved by the harness after spawn and write — you do not need to click them manually.

Guidelines:
- When spawning, default cwd is the project root unless told otherwise.
- Be conservative with kill_pane_process.
- Reply in concise prose. Summarize what panes are doing for the user.

When the task is done, briefly summarize what was accomplished.`;

export async function runPuppetMasterLoop(
  model: LlmModel,
  apiKey: string,
  executor: McpToolExecutor,
  history: ChatMessage[],
  userPrompt: string,
  cb: PuppetMasterCallbacks,
  signal: AbortSignal,
): Promise<void> {
  const livePanes = await executor.listPanes();
  const paneSnapshot =
    livePanes.length > 0
      ? `\n\n[Current open panes — reuse these; do not spawn duplicates]\n${formatPaneList(livePanes)}`
      : '';

  const messages: ChatMessage[] = [
    ...history,
    { role: 'user', content: userPrompt + paneSnapshot },
  ];
  let turns = 0;
  const MAX_TURNS = 12;

  while (turns < MAX_TURNS) {
    if (signal.aborted) return;
    turns++;

    let resp: LlmResponse;
    try {
      resp = await streamLlm(
        model.provider,
        apiKey,
        {
          model: model.model_id,
          system: SYSTEM_PROMPT,
          messages,
          tools: PUPPET_MASTER_TOOLS,
        },
        {
          onTextDelta: (t) => cb.onAssistantText(t),
          signal,
        },
      );
    } catch (err) {
      cb.onError(err instanceof Error ? err : new Error(String(err)));
      return;
    }

    messages.push({ role: 'assistant', content: resp.content, stop_reason: resp.stop_reason });

    // If no tool calls, we're done.
    const toolBlocks = resp.content.filter((b) => b.type === 'tool_use');
    if (toolBlocks.length === 0) {
      cb.onComplete();
      return;
    }

    // Execute each tool call sequentially (could parallelize later).
    const toolResults: Array<{ type: 'tool_result'; tool_use_id: string; content: string; is_error?: boolean }> = [];
    for (const block of toolBlocks) {
      if (block.type !== 'tool_use') continue;
      if (signal.aborted) return;
      try {
        const result = await executeMcpTool(
          executor,
          block.name,
          block.input as Record<string, unknown>,
          (entry) => {
            cb.onToolCall(block.name, block.input, entry.result_preview, entry.error);
          },
        );
        toolResults.push({ type: 'tool_result', tool_use_id: block.id, content: result });
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        toolResults.push({ type: 'tool_result', tool_use_id: block.id, content: msg, is_error: true });
        cb.onToolCall(block.name, block.input, undefined, msg);
      }
    }

    messages.push({ role: 'user', content: toolResults });
  }

  cb.onComplete();
}
