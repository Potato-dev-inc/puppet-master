// Messages POSTed from the mobile PWA through the Rust bridge: run them on the active orchestrator
// backend and stream text / tool results / completion back to the phone as chat events.
import { useEffect } from 'react';
import type { LlmModel, OrchestratorChatEvent } from '@puppet-master/shared';
import type { BridgeClient } from '../../lib/bridge';
import { makeBridgeExecutor, makeTauriExecutor } from '../../lib/mcp-executor';
import { isCliOrchestratorBackend } from '../../lib/orchestrator-panes';
import { runPuppetMasterLoop } from '../../lib/puppet-master';
import { runPuppetMasterCliLoop } from '../../lib/puppet-master-cli';
import { getApiKey, loadSettings } from '../../lib/settings';
import { tauri } from '../../lib/tauri';
import { appendStreamedText, builtinToolLog, prependLogs, type UiMessage } from './chat-model';
import type { OrchestratorSession } from './useOrchestratorSession';

const pushChat = (event: OrchestratorChatEvent) => {
  void tauri.pushChatEvent(JSON.stringify(event));
};

export function useRemoteOrchestratorMessages(
  bridge: BridgeClient | null,
  model: LlmModel,
  session: OrchestratorSession,
) {
  const { abortRef, setMessages, setLogs, setBusy, setStandbyPanes, logAutoApprove } = session;

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;

    const processRemoteMessage = async (msg: { text: string; message_id: string }) => {
      if (cancelled) return;
      const settings = await loadSettings();
      const activeBackend = settings.orchestrator_backend ?? 'api';

      const userUiMsg: UiMessage = { id: msg.message_id, role: 'user', text: msg.text };
      setMessages((m) =>
        m.some((entry) => entry.id === msg.message_id) ? m : [...m, userUiMsg],
      );

      if (activeBackend === 'api') {
        const apiKey = getApiKey(settings, model.provider);
        if (!apiKey) {
          const errText = `No ${model.provider} API key set. Open Settings on desktop.`;
          setMessages((prev) => [
            ...prev,
            { id: crypto.randomUUID(), role: 'system', text: errText },
          ]);
          pushChat({ type: 'error', message_id: msg.message_id, error: errText });
          return;
        }

        const streamId = `streaming-${msg.message_id}`;
        setMessages((m) => [...m, { id: streamId, role: 'assistant', text: '' }]);
        setBusy(true);

        const controller = new AbortController();
        abortRef.current = controller;

        await runPuppetMasterLoop(
          model,
          apiKey,
          bridge ? makeBridgeExecutor(bridge) : makeTauriExecutor(),
          [],
          msg.text,
          {
            onAssistantText: (t) => {
              setMessages((m) => appendStreamedText(m, streamId, t));
              pushChat({ type: 'text', message_id: msg.message_id, text: t });
            },
            onToolCall: (tool, args, result, error) => {
              setLogs((prev) => prependLogs(prev, [builtinToolLog(tool, args, result, error)]));
              pushChat({ type: 'tool', message_id: msg.message_id, tool, result, error });
            },
            onComplete: () => {
              setBusy(false);
              setStandbyPanes([]);
              abortRef.current = null;
              pushChat({ type: 'done', message_id: msg.message_id });
            },
            onError: (err) => {
              setMessages((m) => [
                ...m,
                { id: crypto.randomUUID(), role: 'system', text: `LLM error: ${err.message}` },
              ]);
              setBusy(false);
              setStandbyPanes([]);
              abortRef.current = null;
              pushChat({ type: 'error', message_id: msg.message_id, error: err.message });
            },
            onStandby: (running) => setStandbyPanes(running),
            onAutoApprove: logAutoApprove,
          },
          controller.signal,
        );
        return;
      }

      if (isCliOrchestratorBackend(activeBackend)) {
        setBusy(true);
        const streamId = `streaming-${msg.message_id}`;
        setMessages((m) => [...m, { id: streamId, role: 'assistant', text: '' }]);
        const controller = new AbortController();
        abortRef.current = controller;

        await runPuppetMasterCliLoop(activeBackend, bridge, msg.text, {
          onAssistantText: (t) => {
            setMessages((m) => appendStreamedText(m, streamId, t));
            pushChat({ type: 'text', message_id: msg.message_id, text: t });
          },
          onToolCall: (tool, args, result, error) => {
            setLogs((prev) => prependLogs(prev, [builtinToolLog(tool, args, result, error)]));
            pushChat({ type: 'tool', message_id: msg.message_id, tool, result, error });
          },
          onComplete: () => {
            setBusy(false);
            abortRef.current = null;
            pushChat({ type: 'done', message_id: msg.message_id });
          },
          onError: (err) => {
            setMessages((m) => [
              ...m,
              { id: crypto.randomUUID(), role: 'system', text: err.message },
            ]);
            setBusy(false);
            abortRef.current = null;
            pushChat({ type: 'error', message_id: msg.message_id, error: err.message });
          },
        }, controller.signal);
      }
    };

    tauri.onOrchestratorMessage((msg) => {
      void processRemoteMessage(msg);
    }).then((fn) => { unlisten = fn; }).catch(() => {});

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [bridge, model, abortRef, setMessages, setLogs, setBusy, setStandbyPanes, logAutoApprove]);
}
