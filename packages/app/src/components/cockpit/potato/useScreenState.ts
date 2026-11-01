// The sandbox screen's state (who has the computer), read now and again whenever the app says it moved. Used by the
// thread's "Needs you" banner, which must work while the bot panel (and its Computer tab) is closed.
import { useEffect, useState } from 'react';
import type { ScreenState, ScreenTransport } from '../../../lib/potatoScreen';

/** Null until the first read, and when the app cannot say. */
export function useScreenState(transport: ScreenTransport | undefined): ScreenState | null {
  const [state, setState] = useState<ScreenState | null>(null);
  useEffect(() => {
    if (!transport) {
      setState(null);
      return;
    }
    let live = true;
    let off: (() => void) | undefined;
    const reload = () => {
      void transport.state().then(
        (next) => {
          if (live) setState(next);
        },
        () => {
          if (live) setState(null);
        },
      );
    };
    reload();
    void transport.onChanged?.(reload).then((unlisten) => {
      if (live) off = unlisten;
      else unlisten();
    });
    return () => {
      live = false;
      off?.();
    };
  }, [transport]);
  return state;
}
