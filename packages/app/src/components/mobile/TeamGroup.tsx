// "Team" group of the phone Agents screen: what teammates pushed or opened, and my own queued
// updates, under the agents' questions. Same rules as the desktop card: at most two buttons, the
// ones this phone's pairing allows (`can`), and every action that changes a branch asks first.
// A teammate's words are untrusted data: they are rendered as plain text, in quotes, nothing more.
import { useCallback, useEffect, useRef, useState } from 'react';
import type { BridgeClient, TeamCardsReply } from '../../lib/bridge-types';
import { ago } from '../../lib/mobile-agent-tabs';
import { cardActions, cardMeta, cardTitle, jobSub, jobTitle, type TeamCard, type TeamJob } from '../../lib/team';
import { ConfirmSheet } from './MobileSheet';

/** The bridge is polled; there is no team event on the phone's stream yet. */
const POLL_MS = 15_000;

type Confirm =
  | { kind: 'queue'; card: TeamCard; paneId: string; agent: string }
  | { kind: 'apply'; job: TeamJob };

/** The plain sentence for a failed call. The bridge puts `{ error }` after the status code. */
function plainError(err: unknown): string {
  const text = err instanceof Error ? err.message : String(err);
  const m = / -> (\d+): ([\s\S]*)$/.exec(text);
  if (!m) return "Couldn't reach your PC. Try again.";
  if (m[1] === '403') return "This phone isn't allowed to do that. Change it on your PC: Settings › Phone.";
  try {
    const body = JSON.parse(m[2]) as { error?: unknown };
    if (typeof body.error === 'string' && body.error && !/^[a-z_]+$/.test(body.error)) return body.error;
  } catch {
    // Not JSON: fall through to the generic sentence.
  }
  return "That didn't work. Try again.";
}

export function TeamGroup({ bridge, bridgeReady }: { bridge: BridgeClient; bridgeReady: boolean }) {
  const [reply, setReply] = useState<TeamCardsReply | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const [confirm, setConfirm] = useState<Confirm | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(true);

  const load = useCallback(async () => {
    if (!bridge.teamCards) return;
    try {
      const next = await bridge.teamCards();
      if (!alive.current) return;
      setReply(next);
      setNow(Date.now());
    } catch {
      // An old bridge or a blip: keep what is shown. The group is optional.
    }
  }, [bridge]);

  useEffect(() => {
    alive.current = true;
    if (!bridgeReady || !bridge.teamCards) return;
    void load();
    const timer = window.setInterval(() => void load(), POLL_MS);
    const onVisibility = () => {
      if (document.visibilityState === 'visible') void load();
    };
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      alive.current = false;
      window.clearInterval(timer);
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [bridge, bridgeReady, load]);

  if (!bridge.teamCards || !reply || !reply.enabled) return null;

  const cards = reply.cards.filter((c) => c.state !== 'dismissed');
  const jobs = reply.jobs.filter((j) => j.state !== 'applied' && j.state !== 'discarded');
  const readyJobs = jobs.filter((j) => j.state === 'ready');
  const otherJobs = jobs.filter((j) => j.state !== 'ready');
  if (cards.length === 0 && jobs.length === 0) return null;
  const { can } = reply;

  const run = async (action: () => Promise<{ ok: boolean; result?: unknown }>) => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const res = await action();
      if (typeof res.result === 'string') setNotice(res.result);
    } catch (err) {
      setError(plainError(err));
    } finally {
      setBusy(false);
      void load();
    }
  };

  const jobRow = (job: TeamJob) => {
    const sub = jobSub(job);
    return (
      <li key={`job-${job.id}`} className="ph-team" data-job-id={job.id}>
        <div className="ph-team-title">{jobTitle(job)}</div>
        {sub && <div className="ph-team-meta">{sub}</div>}
        {job.state === 'ready' && can.act && bridge.teamApply && bridge.teamDiscard && (
          <div className="ph-team-btns">
            <button type="button" className="ph-btn ph-btn--primary" disabled={busy} onClick={() => setConfirm({ kind: 'apply', job })}>
              Apply to my branch
            </button>
            <button type="button" className="ph-btn" disabled={busy} onClick={() => void run(() => bridge.teamDiscard!(job.id))}>
              Discard
            </button>
          </div>
        )}
      </li>
    );
  };

  const cardRow = (card: TeamCard) => {
    const actions = cardActions(card);
    const pane = card.panes[0];
    const hasJob = reply.jobs.some((j) => j.cardId === card.id);
    let primary: 'pull' | 'queue' | null = null;
    if (can.act && card.state === 'new') {
      if (card.local?.canPull && bridge.teamPull) primary = 'pull';
      else if (card.local?.canQueue && pane && bridge.teamQueue) primary = 'queue';
    }
    const showDismiss = can.dismiss && !!bridge.teamDismiss;
    let status: string | null = null;
    if (card.state === 'pulled') status = actions.note;
    else if (card.state === 'queued') status = hasJob ? null : actions.note;
    else if (can.act && !primary) {
      if (!card.local) status = "This project isn't open on your PC.";
      else status = card.local.canQueue && !pane ? 'No agent is open in this project.' : actions.note;
    }
    const when = ago(card.receivedAtMs, now);
    const note = card.event.note?.trim();
    return (
      <li key={`card-${card.id}`} className="ph-team" data-card-id={card.id}>
        <div className="ph-team-head">
          <div className="ph-team-title">{cardTitle(card.event)}</div>
          {when && <span className="ph-team-when">{when}</span>}
        </div>
        <div className="ph-team-meta">{cardMeta(card.event)}</div>
        {note && <p className="ph-team-note">{`“${note}”`}</p>}
        {status && <div className="ph-team-meta">{status}</div>}
        {(primary || showDismiss) && (
          <div className="ph-team-btns">
            {primary === 'pull' && (
              <button type="button" className="ph-btn ph-btn--primary" disabled={busy} onClick={() => void run(() => bridge.teamPull!(card.id))}>
                Pull now
              </button>
            )}
            {primary === 'queue' && pane && (
              <button
                type="button"
                className="ph-btn ph-btn--primary"
                disabled={busy}
                onClick={() => setConfirm({ kind: 'queue', card, paneId: pane.paneId, agent: pane.label })}
              >
                Queue my agent
              </button>
            )}
            {showDismiss && (
              <button type="button" className="ph-btn" disabled={busy} onClick={() => void run(() => bridge.teamDismiss!(card.id))}>
                Dismiss
              </button>
            )}
          </div>
        )}
      </li>
    );
  };

  const doConfirm = () => {
    const c = confirm;
    setConfirm(null);
    if (!c) return;
    if (c.kind === 'queue') void run(() => bridge.teamQueue!(c.card.id, c.paneId, 'idle'));
    else void run(() => bridge.teamApply!(c.job.id));
  };

  return (
    <section aria-label="Team">
      <h2 className="ph-sec">Team</h2>
      <ul className="ph-group">
        {readyJobs.map(jobRow)}
        {cards.map(cardRow)}
        {otherJobs.map(jobRow)}
      </ul>
      {notice && (
        <p className="ph-foot" role="status">
          {notice}
        </p>
      )}
      {error && <p className="ph-foot ph-foot--err">{error}</p>}
      {reply.missing > 0 && <p className="ph-foot">Some team updates may be missing.</p>}
      {!can.act && !can.dismiss && (
        <p className="ph-foot">This phone can see team updates but not act on them. Change it on your PC: Settings › Phone.</p>
      )}
      {confirm?.kind === 'queue' && (
        <ConfirmSheet
          title="Queue my agent"
          message={`Queue ${confirm.agent} to bring ${confirm.card.event.subject.branch ?? 'their branch'} into ${confirm.card.local?.currentBranch ?? 'your branch'}? It works in a separate copy and asks you before anything lands.`}
          confirmLabel="Queue it"
          onConfirm={doConfirm}
          onCancel={() => setConfirm(null)}
        />
      )}
      {confirm?.kind === 'apply' && (
        <ConfirmSheet
          title="Apply to my branch"
          message={`Fast-forward ${confirm.job.into}? Nothing is pushed.`}
          confirmLabel="Apply"
          onConfirm={doConfirm}
          onCancel={() => setConfirm(null)}
        />
      )}
    </section>
  );
}
