// What to tell the person about the address their phone will be sent to (shown next to the QR).

export interface OriginNotice {
  /** The host the phone will connect to, shown prominently. */
  host: string;
  /** A short warning, or null when nothing looks off. */
  warning: string | null;
}

function isIpHost(host: string): boolean {
  return /^\d{1,3}(\.\d{1,3}){3}$/.test(host) || host.startsWith('[') || host.includes(':');
}

function isLoopbackHost(host: string): boolean {
  return host === 'localhost' || host.startsWith('127.') || host === '[::1]' || host === '::1';
}

export function originNotice(origin: string): OriginNotice | null {
  let url: URL;
  try {
    url = new URL(origin);
  } catch {
    return null;
  }
  const host = url.host;
  const hostname = url.hostname;
  if (url.protocol === 'http:' && !isLoopbackHost(hostname)) {
    return { host, warning: 'This address uses plain http. Anyone on the network can read the phone token. Use https.' };
  }
  if (isLoopbackHost(hostname)) {
    return { host, warning: 'This address only works on this computer, not on your phone.' };
  }
  if (isIpHost(hostname)) {
    return { host, warning: 'This is an IP address. Make sure it is yours and reachable from your phone.' };
  }
  return { host, warning: null };
}
