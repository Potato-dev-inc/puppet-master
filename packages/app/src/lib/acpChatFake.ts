// A stand-in for `AcpChatApi` for tests and the browser preview: every call answers with something harmless. A test
// passes only the calls it cares about. Not imported by the app.
import type { AcpChatApi, AcpChatState, AcpLaneView } from './acpChat';

export const NO_LANES: AcpLaneView = { labs_enabled: false, agents: [] };

export function fakeAcpApi(over: Partial<AcpChatApi> = {}): AcpChatApi {
  const nothing = async () => null as unknown as AcpChatState;
  return {
    state: async () => null,
    cancel: async () => true,
    watch: async () => () => undefined,
    setConfig: nothing,
    setAgentAnswering: nothing,
    openInTerminal: async () => {
      throw new Error('ACP_UNAVAILABLE: not in this test.');
    },
    backToChat: async () => {
      throw new Error('ACP_UNAVAILABLE: not in this test.');
    },
    conversationOfTerminal: async () => null,
    watchConversations: async () => () => undefined,
    laneGet: async () => NO_LANES,
    laneSet: async () => NO_LANES,
    start: async () => {
      throw new Error('ACP_UNAVAILABLE: not in this test.');
    },
    ...over,
  };
}
