// The Routines tab of the bot panel. The routines phase replaces this file; its props stay.
import { PANEL_COPY } from '../../../lib/potato-panel';
import { ComingSoon } from './ComingSoon';

export function RoutinesTab(_props: { bot: { id: string; name: string } }) {
  return <ComingSoon line={PANEL_COPY.routines} />;
}
