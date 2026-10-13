// "Undo last turn" from the palette. It does not restore silently: it finds the start of the last
// turn, then asks with the same sheet the History tab uses (shared-folder warning included).
import { useEffect, useRef, useState } from 'react';
import { cockpitApi, type Checkpoint, type CheckpointsApi } from '../../lib/cockpit-api';
import { lastTurnStart } from './history-model';
import { RestoreSheet } from './RestoreSheet';
import { restoreWithUndo, type Notify } from './restore-flow';

export function UndoSheet({
  paneId,
  sharedWith,
  notify,
  onClose,
  api = cockpitApi,
}: {
  paneId: string;
  sharedWith: string[];
  notify: Notify;
  onClose: () => void;
  api?: Pick<CheckpointsApi, 'checkpointsList' | 'checkpointRestore'>;
}) {
  const [target, setTarget] = useState<Checkpoint | null>(null);
  const [busy, setBusy] = useState(false);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    api.checkpointsList(paneId).then(
      (listing) => {
        if (!alive.current) return;
        const start = listing.state === 'ok' ? lastTurnStart(listing.items) : undefined;
        if (start) {
          setTarget(start);
          return;
        }
        notify(listing.state === 'ok' ? "There's nothing to undo yet." : listing.message || 'Checkpoints are not available.');
        onClose();
      },
      (e) => {
        if (!alive.current) return;
        notify(e instanceof Error ? e.message : String(e));
        onClose();
      },
    );
    return () => {
      alive.current = false;
    };
    // Load once per open: notify/onClose identities change every render of the parent.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api, paneId]);

  if (!target) return null;
  return (
    <RestoreSheet
      title="Undo the last turn?"
      confirmLabel="Undo last turn"
      sharedWith={sharedWith}
      busy={busy}
      onCancel={onClose}
      onConfirm={() => {
        setBusy(true);
        void restoreWithUndo(api, paneId, target, notify).finally(() => {
          if (alive.current) setBusy(false);
          onClose();
        });
      }}
    />
  );
}
