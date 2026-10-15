// The order of the Inbox: one list, not one box per kind. High risk and consent first, then calmer
// approvals, questions, then pauses and stops; ties go to the oldest. This is the Rust rule
// (src-tauri/src/attention/model.rs `rank_of`), so the desktop list, the phone list and a push agree.

export type InboxKind = 'consent' | 'approval' | 'question' | 'budget' | 'limit' | 'error' | 'stuck';
export type InboxRisk = 'low' | 'medium' | 'high' | undefined;

/** Lower comes first. */
export function inboxRank(kind: InboxKind, risk?: InboxRisk): number {
  switch (kind) {
    case 'consent':
      return 0;
    case 'approval':
      return risk === 'high' ? 0 : risk === 'low' ? 2 : 1;
    case 'question':
      return 2;
    case 'budget':
    case 'limit':
      return 3;
    case 'error':
    case 'stuck':
      return 4;
  }
}

export interface InboxEntry {
  key: string;
  kind: InboxKind;
  risk?: InboxRisk;
  /** Epoch ms since when it has waited, when known. */
  since?: number;
}

/** Rank, then oldest first (an unknown age goes last), then key, so the order never flickers. */
export function sortInbox<T extends InboxEntry>(entries: ReadonlyArray<T>): T[] {
  return [...entries].sort(
    (a, b) =>
      inboxRank(a.kind, a.risk) - inboxRank(b.kind, b.risk) ||
      (a.since ?? Number.MAX_SAFE_INTEGER) - (b.since ?? Number.MAX_SAFE_INTEGER) ||
      a.key.localeCompare(b.key),
  );
}
