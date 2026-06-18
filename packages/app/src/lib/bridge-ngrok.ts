/** ngrok free tier shows an HTML interstitial unless this request header is sent. */
export const NGROK_SKIP_BROWSER_WARNING = 'ngrok-skip-browser-warning';

export function isNgrokHost(url: string): boolean {
  try {
    return new URL(url).hostname.includes('ngrok');
  } catch {
    return false;
  }
}

export function ngrokRequestHeaders(baseUrl: string): Record<string, string> {
  if (!isNgrokHost(baseUrl)) return {};
  return { [NGROK_SKIP_BROWSER_WARNING]: '1' };
}
