// App bridge: the local server other tools and agents talk to. Running = the app has discovered its address.
import type { CheckRow, SetupCheckContext } from './types';

export function checkBridge(ctx: SetupCheckContext): CheckRow[] {
  return [
    ctx.bridgeUrl
      ? { id: 'bridge', status: 'ok', sentence: 'Bridge is running', fix: null }
      : { id: 'bridge', status: 'fail', sentence: 'Bridge is not running. Close Puppet Master and open it again.', fix: null },
  ];
}
