// Kept for one release: the push plumbing grew card deep links and moved to `usePushIntents`.
import { usePushIntents } from './usePushIntents';

export function usePushPresence(
  bridgeUrl: string | null,
  onOpenPane: (paneId: string) => void,
  onOpenQuestions?: () => void,
): void {
  usePushIntents(bridgeUrl, { onOpenPane, onOpenQuestions });
}
