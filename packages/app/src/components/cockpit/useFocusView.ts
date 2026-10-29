// The Focus view for the cockpit: read once, then kept current by `focus://changed`. Null until it is read, and
// stays null on a page with no desktop shell (the read fails quietly and nothing shows).
import { useEffect, useState } from 'react';
import { focusState, onFocusChanged, type FocusView } from '../../lib/focus';

export function useFocusView(): [FocusView | null, (view: FocusView) => void] {
  const [view, setView] = useState<FocusView | null>(null);
  useEffect(() => {
    let live = true;
    let unlisten: (() => void) | undefined;
    Promise.resolve()
      .then(() => focusState())
      .then((v) => {
        if (live && v) setView(v);
      })
      .catch(() => undefined);
    onFocusChanged((v) => {
      if (live) setView(v);
    })
      .then((off) => {
        if (live) unlisten = off;
        else off();
      })
      .catch(() => undefined);
    return () => {
      live = false;
      unlisten?.();
    };
  }, []);
  return [view, setView];
}
