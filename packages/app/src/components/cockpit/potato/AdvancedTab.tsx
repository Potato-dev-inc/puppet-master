// The Advanced tab of the bot panel (per-bot settings). Notifications, Open work and Memory are their own components
// (P7); the settings phase after them adds its sections below. The props stay as they were.
import { MemoryList } from './MemoryList';
import { NotifySection } from './NotifySection';
import { OpenWork } from './OpenWork';
import '../../../styles/cockpit-potato-settings.css';

export function AdvancedTab({ bot }: { bot: { id: string; name: string } }) {
  return (
    <div className="ck-potato-advanced">
      <NotifySection botId={bot.id} />
      <OpenWork botId={bot.id} />
      <MemoryList botId={bot.id} />
    </div>
  );
}
