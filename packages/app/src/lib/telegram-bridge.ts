// The phone's two Telegram routes on the bridge (`bridge/routes/channels.rs`), modelled on
// `push-client.ts`. The phone can read whether Telegram is on and switch it off, nothing more:
// there is no call here that turns it on, pairs a chat or changes what it may do.
import { ngrokRequestHeaders } from './bridge-ngrok';
import { mergeBridgeHeaders } from './mobile-pairing-auth';

/** `GET /channels/telegram`: whether the chat is on, connected, and the bot's `@name`. Nothing else. */
export interface TelegramPhoneStatus {
  on: boolean;
  connected: boolean;
  botUsername: string | null;
}

/** `POST /channels/telegram/off`: `wasOn` says whether it was on before the call. */
export interface TelegramOffResult {
  ok: boolean;
  wasOn: boolean;
}

async function request<T>(baseUrl: string, method: 'GET' | 'POST', path: string): Promise<T> {
  const headers = mergeBridgeHeaders({ ...ngrokRequestHeaders(baseUrl) });
  const res = await fetch(`${baseUrl}${path}`, { method, headers });
  // The error names the route and status only: never the bearer token or the response body.
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status}`);
  return (await res.json()) as T;
}

/** Reads the Telegram state from the PC at `baseUrl`. Missing fields read as "off". */
export async function getTelegramPhoneStatus(baseUrl: string): Promise<TelegramPhoneStatus> {
  const raw = await request<Partial<TelegramPhoneStatus>>(baseUrl, 'GET', '/channels/telegram');
  return {
    on: raw.on === true,
    connected: raw.connected === true,
    botUsername: typeof raw.botUsername === 'string' ? raw.botUsername : null,
  };
}

/** Switches Telegram off on the PC at `baseUrl`. Only a paired phone's token is accepted. */
export async function turnTelegramOff(baseUrl: string): Promise<TelegramOffResult> {
  const raw = await request<Partial<TelegramOffResult>>(baseUrl, 'POST', '/channels/telegram/off');
  return { ok: raw.ok === true, wasOn: raw.wasOn === true };
}
