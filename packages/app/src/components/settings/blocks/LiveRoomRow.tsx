// Settings › General › Live Room: the settings block with the team relay looked up, so the relay field can say
// `Uses your team relay` when the field is empty and a team relay is set.
import { useTeamRelay } from '../../cockpit/live/useTeamRelay';
import { LiveRoomSettings } from './LiveRoomSettings';

export function LiveRoomRow({ onSaved }: { onSaved?: () => void }) {
  const teamRelay = useTeamRelay();
  return <LiveRoomSettings teamRelay={teamRelay} onSaved={onSaved} />;
}
