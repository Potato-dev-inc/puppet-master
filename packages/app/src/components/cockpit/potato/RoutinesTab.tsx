// The Routines tab of the bot panel: the bot's routines as a short list (name, schedule in words, next run, a pause
// switch, Run now, Delete), and "New routine" for a small form. Routines run in the bot's own thread as a message from
// "routine"; the list only shows and changes them. The Advanced tab and the editor come later.
import { useCallback, useEffect, useState } from 'react';
import { plainError } from '../../../lib/potato';
import { tauriTrashTransport, type TrashTransport } from '../../../lib/potato-trash';
import {
  ROUTINE_COPY,
  routineStatusLine,
  tauriRoutinesTransport,
  type RoutinesTransport,
  type RoutineView,
} from '../../../lib/potato-routines';
import '../../../styles/cockpit-potato-routines.css';
import { RoutineForm } from './RoutineForm';
import { UndoToast } from './UndoToast';

/** The line after a routine is deleted (Undo for 10 s; the routine waits in Recently deleted for 7 days). */
export const ROUTINE_DELETED_COPY = 'Routine deleted.';

export interface RoutinesTabProps {
  bot: { id: string; name: string };
  transport?: RoutinesTransport;
  trash?: TrashTransport;
  /** The clock for "next run" words (tests pass a fixed one). */
  now?: number;
}

export function RoutinesTab({ bot, transport = tauriRoutinesTransport, trash = tauriTrashTransport, now }: RoutinesTabProps) {
  const [routines, setRoutines] = useState<RoutineView[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [making, setMaking] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [undo, setUndo] = useState<{ trashId: string } | null>(null);
  const clock = now ?? Date.now();

  const load = useCallback(() => {
    setLoadError(null);
    transport.list(bot.id).then(setRoutines, (e: unknown) => setLoadError(plainError(e, ROUTINE_COPY.loadFailed)));
  }, [bot.id, transport]);

  useEffect(() => {
    setRoutines(null);
    setMaking(false);
    setNotice(null);
    setUndo(null);
    load();
  }, [load]);

  const replace = (next: RoutineView) => setRoutines((list) => (list ?? []).map((r) => (r.id === next.id ? next : r)));
  const drop = (routine: RoutineView, trashId: string) => {
    setRoutines((list) => (list ?? []).filter((r) => r.id !== routine.id));
    setUndo({ trashId });
  };

  const undoDelete = async () => {
    if (!undo) return;
    const { trashId } = undo;
    setUndo(null);
    try {
      await trash.restore(trashId);
      load();
    } catch (e) {
      setLoadError(plainError(e, ROUTINE_COPY.actionFailed));
    }
  };

  return (
    <section className="ck-potato-routines" aria-label={ROUTINE_COPY.title}>
      <div className="ck-potato-routines-head">
        <span className="ck-potato-routines-title">{ROUTINE_COPY.title}</span>
        {!making ? (
          <button type="button" className="ck-potato-routine-btn" onClick={() => setMaking(true)}>
            {ROUTINE_COPY.newRoutine}
          </button>
        ) : null}
      </div>
      {making ? (
        <RoutineForm
          botId={bot.id}
          transport={transport}
          onCancel={() => setMaking(false)}
          onSaved={(routine) => {
            setMaking(false);
            setRoutines((list) => [...(list ?? []), routine]);
          }}
        />
      ) : null}
      {notice ? (
        <p role="status" className="ck-potato-routines-notice">
          {notice}
        </p>
      ) : null}
      {loadError ? (
        <p role="alert" className="ck-potato-routine-error">
          {loadError}
        </p>
      ) : null}
      {routines === null && !loadError ? <p className="ck-sub ck-potato-routines-empty">Loading...</p> : null}
      {routines && routines.length === 0 && !making ? <p className="ck-sub ck-potato-routines-empty">{ROUTINE_COPY.empty}</p> : null}
      {undo ? <UndoToast message={ROUTINE_DELETED_COPY} onUndo={() => void undoDelete()} onExpire={() => setUndo(null)} /> : null}
      {routines && routines.length > 0 ? (
        <ul className="ck-potato-routines-list">
          {routines.map((routine) => (
            <RoutineRow
              key={routine.id}
              routine={routine}
              botName={bot.name}
              transport={transport}
              line={routineStatusLine(routine, clock)}
              onChanged={replace}
              onDeleted={drop}
              onNotice={setNotice}
            />
          ))}
        </ul>
      ) : null}
    </section>
  );
}

interface RoutineRowProps {
  routine: RoutineView;
  botName: string;
  transport: RoutinesTransport;
  line: string;
  onChanged: (routine: RoutineView) => void;
  onDeleted: (routine: RoutineView, trashId: string) => void;
  onNotice: (text: string | null) => void;
}

function RoutineRow({ routine, botName, transport, line, onChanged, onDeleted, onNotice }: RoutineRowProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fail = (e: unknown) => {
    setError(plainError(e, ROUTINE_COPY.actionFailed));
    setBusy(false);
  };

  const togglePause = () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    transport.pause(routine.id, !routine.paused).then(
      (next) => {
        setBusy(false);
        onChanged(next);
      },
      fail,
    );
  };

  const runNow = () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    transport.runNow(routine.id).then(
      () => {
        setBusy(false);
        onNotice(ROUTINE_COPY.sent(botName));
      },
      fail,
    );
  };

  const remove = () => {
    if (busy) return;
    setBusy(true);
    transport.remove(routine.id).then(
      (trashId) => onDeleted(routine, trashId),
      fail,
    );
  };

  return (
    <li className="ck-potato-routine">
      <div className="ck-potato-routine-main">
        <span className="ck-potato-routine-name">{routine.title}</span>
        <span className="ck-potato-routine-when">{routine.summary}</span>
        <span className="ck-potato-routine-line">{line}</span>
      </div>
      <div className="ck-potato-routine-actions">
        <button
          type="button"
          role="switch"
          aria-checked={!routine.paused}
          aria-label={`${routine.paused ? 'Turn on' : 'Pause'} ${routine.title}`}
          className="ck-potato-routine-switch"
          disabled={busy || routine.status === 'done'}
          onClick={togglePause}
        >
          <span className="ck-potato-routine-knob" aria-hidden="true" />
        </button>
        <button type="button" className="ck-potato-routine-btn" disabled={busy} onClick={runNow}>
          {ROUTINE_COPY.runNow}
        </button>
        <button type="button" className="ck-potato-routine-btn" disabled={busy} onClick={remove}>
          {ROUTINE_COPY.delete}
        </button>
      </div>
      {error ? (
        <p role="alert" className="ck-potato-routine-error">
          {error}
        </p>
      ) : null}
    </li>
  );
}
