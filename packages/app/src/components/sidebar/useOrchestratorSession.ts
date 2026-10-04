// Chat/run state of the built-in orchestrator: message list, MCP log, busy/standby flags, interrupt/new-chat,
// and the "wake" run that the attention tracker triggers when a worker pane needs help.
import { useCallback, useEffect, useRef, useState } from 'react';
import type { LlmModel, McpLogEntry, OrchestratorBackend } from '@puppet-master/shared';
import type { BridgeClient } from '../../lib/bridge';
import type { ChatMessage } from '../../lib/llm';
import { makeBridgeExecutor, makeTauriExecutor } from '../../lib/mcp-executor';
import { isCliOrchestratorBackend } from '../../lib/orchestrator-panes';
import { runPuppetMasterLoop } from '../../lib/puppet-master';
import { runPuppetMasterCliLoop } from '../../lib/puppet-master-cli';
import { getApiKey, loadSettings } from '../../lib/settings';
import {
  appendStreamedText,
  builtinToolLog,
  prependLogs,
  type UiLogEntry,
  type UiMessage,
} from './chat-model';

interface SessionInputs {
  bridge: BridgeClient | null;
  backend: OrchestratorBackend;
  model: LlmModel;
  externalLogs: McpLogEntry[];
}

export function useOrchestratorSession({ bridge, backend, model, externalLogs }: SessionInputs) {
  const [messages, setMessages] = useState<UiMessage[]>([]);
  const [logs, setLogs] = useState<UiLogEntry[]>([]);
  const [busy, setBusy] = useState(false);
  const [standbyPanes, setStandbyPanes] = useState<Array<{ id: string; status: string }>>([]);
  const abortRef = useRef<AbortController | null>(null);
  const busyRef = useRef(busy);
  const messagesRef = useRef(messages);
  const backendRef = useRef(backend);
  const modelRef = useRef(model);
  const bridgeRef = useRef(bridge);

  useEffect(() => { busyRef.current = busy; }, [busy]);
  useEffect(() => { messagesRef.current = messages; }, [messages]);
  useEffect(() => { backendRef.current = backend; }, [backend]);
  useEffect(() => { modelRef.current = model; }, [model]);
  useEffect(() => { bridgeRef.current = bridge; }, [bridge]);

  const logAutoApprove = useCallback((paneId: string) => {
    setLogs((prev) => prependLogs(prev, [{
      id: crypto.randomUUID(),
      ts: Date.now(),
      source: 'builtin',
      tool: 'auto-approve',
      args: { pane_id: paneId },
      result: 'permission prompt approved',
    }]));
  }, []);

  /** Run the orchestrator on an internally generated prompt (a worker pane needs attention). */
  const runInternalWake = useCallback(async (text: string) => {
    const settings = await loadSettings();
    const activeBackend = settings.orchestrator_backend ?? backendRef.current;
    setMessages((m) => [...m, { id: crypto.randomUUID(), role: 'system', text }]);

    if (activeBackend === 'api') {
      const activeModel = modelRef.current;
      const apiKey = getApiKey(settings, activeModel.provider);
      if (!apiKey) {
        setMessages((m) => [
          ...m,
          { id: crypto.randomUUID(), role: 'system', text: `Worker needs attention, but no ${activeModel.provider} API key is set.` },
        ]);
        return;
      }

      const streamId = 'streaming-' + crypto.randomUUID();
      setMessages((m) => [...m, { id: streamId, role: 'assistant', text: '' }]);
      busyRef.current = true;
      setBusy(true);
      const controller = new AbortController();
      abortRef.current = controller;
      const history: ChatMessage[] = messagesRef.current
        .filter((m) => m.role === 'user' || m.role === 'assistant')
        .map((m): ChatMessage =>
          m.role === 'user'
            ? { role: 'user', content: m.text }
            : { role: 'assistant', content: [{ type: 'text', text: m.text }] },
        );

      await runPuppetMasterLoop(
        activeModel,
        apiKey,
        bridgeRef.current ? makeBridgeExecutor(bridgeRef.current) : makeTauriExecutor(),
        history,
        text,
        {
          onAssistantText: (t) => setMessages((m) => appendStreamedText(m, streamId, t)),
          onToolCall: (tool, args, result, error) => {
            setLogs((prev) => prependLogs(prev, [builtinToolLog(tool, args, result, error)]));
          },
          onComplete: () => {
            busyRef.current = false;
            setBusy(false);
            setStandbyPanes([]);
            abortRef.current = null;
          },
          onError: (err) => {
            setMessages((m) => [
              ...m,
              { id: crypto.randomUUID(), role: 'system', text: `LLM error: ${err.message}` },
            ]);
            busyRef.current = false;
            setBusy(false);
            setStandbyPanes([]);
            abortRef.current = null;
          },
          onStandby: (running) => setStandbyPanes(running),
          onAutoApprove: logAutoApprove,
        },
        controller.signal,
      );
      return;
    }

    if (isCliOrchestratorBackend(activeBackend)) {
      busyRef.current = true;
      setBusy(true);
      const streamId = 'streaming-' + crypto.randomUUID();
      setMessages((m) => [...m, { id: streamId, role: 'assistant', text: '' }]);
      const controller = new AbortController();
      abortRef.current = controller;

      await runPuppetMasterCliLoop(activeBackend, bridgeRef.current, text, {
        onAssistantText: (t) => setMessages((m) => appendStreamedText(m, streamId, t)),
        onToolCall: (tool, args, result, error) => {
          setLogs((prev) => prependLogs(prev, [builtinToolLog(tool, args, result, error)]));
        },
        onComplete: () => {
          busyRef.current = false;
          setBusy(false);
          abortRef.current = null;
        },
        onError: (err) => {
          setMessages((m) => [
            ...m,
            { id: crypto.randomUUID(), role: 'system', text: err.message },
          ]);
          busyRef.current = false;
          setBusy(false);
          abortRef.current = null;
        },
      }, controller.signal);
    }
  }, [logAutoApprove]);

  // Merge MCP calls made by external hosts into the log.
  useEffect(() => {
    if (externalLogs.length === 0) return;
    setLogs((prev) => {
      const known = new Set(prev.map((l) => l.id));
      const fresh: UiLogEntry[] = externalLogs
        .filter((l) => !known.has(l.id))
        .map((l) => ({
          id: l.id,
          ts: l.timestamp,
          source: l.source,
          tool: l.tool,
          args: l.args,
          result: l.result_preview,
          error: l.error,
        }));
      return prependLogs(prev, fresh);
    });
  }, [externalLogs]);

  const interrupt = useCallback(() => {
    abortRef.current?.abort();
    setBusy(false);
    setStandbyPanes([]);
  }, []);

  const newChat = useCallback(() => {
    abortRef.current?.abort();
    setBusy(false);
    setStandbyPanes([]);
    setMessages([]);
    setLogs([]);
  }, []);

  return {
    messages,
    logs,
    busy,
    busyRef,
    standbyPanes,
    abortRef,
    setMessages,
    setLogs,
    setBusy,
    setStandbyPanes,
    logAutoApprove,
    runInternalWake,
    interrupt,
    newChat,
  };
}

export type OrchestratorSession = ReturnType<typeof useOrchestratorSession>;
