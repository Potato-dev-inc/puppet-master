// "Turn Telegram off" row of the phone menu sheet. The phone can only switch Telegram OFF
// (bridge POST /channels/telegram/off). Turning it on, pairing and every other setting stay on the PC.
// The row shows only while the PC says Telegram is on, so a phone with no paired chat never sees it.
import { useEffect, useMemo, useState } from 'react';
import { ngrokRequestHeaders } from '../../lib/bridge-ngrok';
import { mergeBridgeHeaders } from '../../lib/mobile-pairing-auth';

export interface TelegramOffDeps {
  /** GET /channels/telegram. Only `on` matters here: it is false until the chat is paired and turned on. */
  status: () => Promise<{ on: boolean }>;
  /** POST /channels/telegram/off. */
  turnOff: () => Promise<void>;
}

/** Real calls to the paired phone's bridge. Same header rules as `push-client.ts`. */
export function browserTelegramDeps(baseUrl: string): TelegramOffDeps {
  async function call<T>(method: string, path: string): Promise<T> {
    const headers = mergeBridgeHeaders({ ...ngrokRequestHeaders(baseUrl) });
    const res = await fetch(`${baseUrl}${path}`, { method, headers });
    if (!res.ok) throw new Error(`${method} ${path} -> ${res.status}`);
    return (await res.json()) as T;
  }
  return {
    status: () => call<{ on: boolean }>('GET', '/channels/telegram'),
    turnOff: async () => {
      await call('POST', '/channels/telegram/off');
    },
  };
}

export function TelegramOffRow({
  baseUrl,
  bridgeReady,
  deps: injected,
}: {
  baseUrl: string;
  bridgeReady: boolean;
  /** Tests inject fakes. */
  deps?: TelegramOffDeps;
}) {
  const deps = useMemo(() => injected ?? browserTelegramDeps(baseUrl), [injected, baseUrl]);
  // What the PC last said: true only when Telegram is on. A failed read counts as off (row hidden).
  const [on, setOn] = useState(false);
  // This phone switched it off in this session.
  const [done, setDone] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!bridgeReady) return;
    let live = true;
    deps
      .status()
      .then((s) => live && setOn(s.on === true))
      .catch(() => live && setOn(false));
    return () => {
      live = false;
    };
  }, [deps, bridgeReady]);

  if (!on && !done) return null;

  const turnOff = async () => {
    setBusy(true);
    setFailed(false);
    try {
      await deps.turnOff();
      setDone(true);
    } catch {
      setFailed(true);
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <div className="ph-group">
        <div className="ph-switch-row">
          <span className="ph-rowtext">Telegram · {done ? 'Off' : 'On'}</span>
        </div>
        {!done && (
          <button
            type="button"
            disabled={!bridgeReady || busy}
            onClick={() => void turnOff()}
            className="ph-rowbtn ph-rowbtn--strong"
            style={{ borderTop: '1px solid var(--sep)' }}
          >
            Turn Telegram off
          </button>
        )}
      </div>
      {done && <p className="ph-foot">Off. Turn it on again on your PC.</p>}
      {failed && (
        <p role="alert" className="ph-foot ph-foot--err">
          Couldn't turn Telegram off. Your PC didn't answer.
        </p>
      )}
    </>
  );
}
