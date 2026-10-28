// Shows the "Bring my old chats" sheets. Anything can ask for them (the palette, a search hit, the Settings card)
// through the request stores in lib/chatImport.ts; this host watches both and shows the sheet while a request waits.
// Closing a sheet clears its request, so no request means no sheet. "Continue in a pane" starts a pane with exactly the
// backend's plan, selects it and opens its Chat tab.
import { useRef } from 'react';
import type { CockpitApi } from '../../hooks/useCockpit';
import {
  chatImportApi,
  clearBringChatsRequest,
  clearImportedChatRequest,
  useBringChatsRequest,
  useImportedChatRequest,
  type ImportApi,
  type ResumePlan,
} from '../../lib/chatImport';
import { requestChatTab } from '../../lib/chatTab';
import { Sheet } from '../cockpit/Sheet';
import { BringChatsPanel, BRING_CHATS_TITLE_ID } from './BringChatsPanel';
import { ImportedChatSheet } from './ImportedChatSheet';

/**
 * The Bring my old chats panel in a sheet, while it is asked for. A window with no cockpit around it (the Settings
 * window) uses it alone with `standalone`: the sheet styles live under `.cockpit`, so a see-through one wraps it.
 */
export function BringChatsHost({ api = chatImportApi, standalone = false }: { api?: ImportApi; standalone?: boolean }) {
  const request = useBringChatsRequest();
  if (!request) return null;
  const close = () => clearBringChatsRequest(request);
  const sheet = (
    <Sheet labelledBy={BRING_CHATS_TITLE_ID} role="dialog" onClose={close}>
      <BringChatsPanel api={api} onClose={close} />
    </Sheet>
  );
  return standalone ? (
    <div className="cockpit" data-testid="bring-chats-standalone" style={{ background: 'transparent', display: 'block' }}>
      {sheet}
    </div>
  ) : (
    sheet
  );
}

type PaneHost = Pick<CockpitApi, 'select' | 'notify'> & { registry: Pick<CockpitApi['registry'], 'spawnPane'> };

/** Both sheets, for the cockpit window, which has the panes to continue an old chat in. */
export function ImportHost({ c, api = chatImportApi }: { c: PaneHost; api?: ImportApi }) {
  const request = useImportedChatRequest();
  const starting = useRef(false);
  if (!request) return <BringChatsHost api={api} />;

  const close = () => clearImportedChatRequest(request);
  const onContinue = (plan: ResumePlan) => {
    if (starting.current) return;
    starting.current = true;
    c.registry
      .spawnPane({ agent_type: plan.agentType, cwd: plan.cwd, extra_args: plan.extraArgs })
      .then((paneId) => {
        // The Chat tab is asked for first: the pane view opens on it when it is shown.
        requestChatTab(paneId);
        c.select(paneId);
        close();
      })
      .catch((error: unknown) => c.notify(`Could not start the agent: ${error instanceof Error ? error.message : String(error)}`))
      .finally(() => {
        starting.current = false;
      });
  };

  return (
    <>
      <ImportedChatSheet request={request} api={api} onContinue={onContinue} onClose={close} />
      <BringChatsHost api={api} />
    </>
  );
}
