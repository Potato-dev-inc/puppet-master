// The bot's mood as plain text: "Genki · energy 72" (spec section 4). Nothing is drawn but the words.
import { moodLine, type MoodNow } from '../../../lib/potato';

export function MoodChip({ mood }: { mood: MoodNow | null }) {
  if (!mood) return null;
  return (
    <span className="ck-potato-mood" title="How it feels right now">
      {moodLine(mood)}
    </span>
  );
}
