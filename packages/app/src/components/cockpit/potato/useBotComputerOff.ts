// Whether this bot's Computer tool switch (Advanced > Tools) is off. The Computer tab shows a line about it. The switch is
// read again every time the Computer tab is shown, so a change on the Advanced tab is there when the person comes back.
import { useEffect, useState } from 'react';
import { tauriBotSettingsTransport, type BotSettingsTransport } from '../../../lib/potato-botsettings';

export function useBotComputerOff(botId: string, active: boolean, transport: BotSettingsTransport = tauriBotSettingsTransport): boolean {
  const [off, setOff] = useState(false);
  useEffect(() => {
    if (!active) return;
    let live = true;
    transport
      .options(botId)
      .then((options) => {
        if (live) setOff(!options.tools.computer);
      })
      .catch(() => {
        if (live) setOff(false);
      });
    return () => {
      live = false;
    };
  }, [botId, active, transport]);
  return off;
}
