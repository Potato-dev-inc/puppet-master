// Review Desk on the phone (SPEC n33 3.9): the read-only summary from `GET /review-desk`. The PC answers
// `view::phone_view`: counts and at most 20 rows of words and numbers. No keys, pane ids, project names,
// paths, file names or diffs, and the titles are already redacted and cut on the PC. Keep, Ask to fix and
// Throw away are on the computer only, so this file has no action call.
// This file must not import the desktop `review-desk.ts` (it pulls in Tauri). Headers match the other phone reads.
import { ngrokRequestHeaders } from './bridge-ngrok';
import { localSecretHeaders } from './bridge-local-secret';
import { mergeBridgeHeaders } from './mobile-pairing-auth';

/** One row of the phone's list (camelCase, as the PC sends it). */
export interface PhoneReviewItem {
  title: string;
  /** `High risk`, `Check first` or `Low risk` (the PC's words). */
  levelWord: string;
  agentLabel: string;
  /** `Copy`, `Mission`, `Night Shift`, `Issue` or `Folder`. */
  whereWord: string;
  files: number;
  added: number;
  deleted: number;
}

/** `GET /review-desk`: the count of items to review, how many are high risk, and up to 20 rows. */
export interface PhoneReviewView {
  count: number;
  high: number;
  items: PhoneReviewItem[];
}

/** The phone's one Review Desk call. Resolves the summary; throws when the PC refuses or cannot answer. */
export interface ReviewPhoneApi {
  read: () => Promise<PhoneReviewView>;
}

/** The phone's read of the desk, with the same headers as the other bridge reads. */
export function bridgeReviewApi(baseUrl: string): ReviewPhoneApi {
  const headers = (): Record<string, string> =>
    mergeBridgeHeaders({ ...ngrokRequestHeaders(baseUrl), ...localSecretHeaders(baseUrl) });
  return {
    read: async () => {
      const res = await fetch(`${baseUrl}/review-desk`, { headers: headers() });
      if (!res.ok) throw new Error(`bridge GET /review-desk -> ${res.status}`);
      return (await res.json()) as PhoneReviewView;
    },
  };
}

/** The mark of a row: `●` High risk, `◐` Check first, `○` Low risk (the same marks as the desk). */
export function reviewGlyph(levelWord: string): string {
  if (levelWord === 'High risk') return '●';
  if (levelWord === 'Check first') return '◐';
  return '○';
}

/** The summary line: `4 to review · 1 high risk`. The high-risk part is left out when there is none. */
export function reviewSummaryLine(view: Pick<PhoneReviewView, 'count' | 'high'>): string {
  const base = `${view.count} to review`;
  return view.high > 0 ? `${base} · ${view.high} high risk` : base;
}
