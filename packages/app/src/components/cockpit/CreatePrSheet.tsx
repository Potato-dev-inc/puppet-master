// "Create a draft pull request" from a separate copy's Changes tab. Opens with a title and body made
// from the receipt (what changed, checks, second opinion). When `gh` is missing, not signed in, or
// the project has no GitHub remote, it says so in plain words and offers the command to copy.
import { useEffect, useState } from 'react';
import type { WorktreeItem } from '../../lib/cockpit-api';
import { shipApi, type PrPrepared, type PrRef, type ShipApi } from '../../lib/ship-api';
import { cannotCreate, copyCommands } from '../../lib/ship-model';
import { Sheet } from './Sheet';
import '../../styles/cockpit-ship.css';

const errText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

export function CreatePrSheet({
  item,
  ship = shipApi,
  agentName,
  onClose,
  onDone,
}: {
  item: WorktreeItem;
  ship?: ShipApi;
  /** Display name of the agent that made the changes ("Codex"). */
  agentName?: string;
  onClose: () => void;
  onDone: (pr: PrRef, updated: boolean) => void;
}) {
  const [prepared, setPrepared] = useState<PrPrepared | null>(null);
  const [title, setTitle] = useState('');
  const [body, setBody] = useState('');
  const [base, setBase] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    let live = true;
    ship
      .prPrepare(item.repo, item.handle)
      .then((p) => {
        if (!live) return;
        setPrepared(p);
        setTitle(p.title);
        setBody(p.body);
        setBase(p.base);
      })
      .catch((err) => live && setError(errText(err)));
    return () => {
      live = false;
    };
  }, [ship, item.repo, item.handle]);

  const blocked = prepared ? cannotCreate(prepared) : null;
  const updating = prepared?.existing != null;

  const submit = async () => {
    if (!prepared) return;
    setBusy(true);
    setError(null);
    try {
      const pr = await ship.prCreate(item.repo, item.handle, title, body, base);
      onDone(pr, updating);
    } catch (err) {
      setError(errText(err));
      setBusy(false);
    }
  };

  const command = prepared ? copyCommands(prepared.branch, base || prepared.base, title || prepared.title) : '';
  const copy = () => {
    void navigator.clipboard
      ?.writeText(blocked?.command ?? command)
      .then(() => setCopied(true))
      .catch(() => undefined);
  };

  return (
    <Sheet labelledBy="ck-pr-t" role="dialog" className="ck-sheet--pr" onClose={busy ? undefined : onClose}>
      <h2 className="ck-section" id="ck-pr-t">
        {updating ? `Update pull request #${prepared?.existing?.number}` : 'Create a draft pull request'}
      </h2>

      {!prepared && !error && <p className="ck-sub">Looking at your GitHub setup…</p>}

      {blocked && (
        <>
          <p className="ck-sub" role="status">
            {blocked.text}
          </p>
          {(blocked.command || prepared?.gh === 'missing') && (
            <>
              <p className="ck-sub">{blocked.command ? 'Command' : 'Or do it by hand'}</p>
              <pre className="ck-pr-command">{blocked.command ?? command}</pre>
              <button type="button" className="ck-btn ck-btn--secondary ck-pr-copy" onClick={copy}>
                {copied ? 'Copied' : 'Copy the command'}
              </button>
            </>
          )}
        </>
      )}

      {prepared && !blocked && (
        <>
          {updating ? (
            <p className="ck-sub">
              Pushes the new commits on <code>{prepared.branch}</code> to GitHub. The pull request keeps its title and text.
            </p>
          ) : (
            <>
              <label className="ck-pr-field">
                <span>Title</span>
                <input className="ck-pr-input" value={title} onChange={(e) => setTitle(e.target.value)} disabled={busy} />
              </label>
              <label className="ck-pr-field">
                <span>Text, made from the receipt</span>
                <textarea
                  className="ck-pr-body"
                  value={body}
                  onChange={(e) => setBody(e.target.value)}
                  disabled={busy}
                  spellCheck={false}
                  rows={9}
                />
              </label>
              <label className="ck-pr-field ck-pr-field--inline">
                <span>Base</span>
                <input className="ck-pr-input ck-pr-input--short" value={base} onChange={(e) => setBase(e.target.value)} disabled={busy} />
              </label>
            </>
          )}
          <p className="ck-sub ck-pr-fine">
            Puppet Master pushes this copy&apos;s changes to GitHub as the branch <code>{prepared.branch}</code>. It never changes{' '}
            {base || 'main'} and never overwrites a branch.
            {prepared.has_uncommitted && ` It saves ${agentName ?? 'the agent'}'s changes as one commit first.`}
          </p>
        </>
      )}

      {error && (
        <p className="ck-changes-error" role="alert">
          {error}
        </p>
      )}

      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn ck-btn--secondary" onClick={onClose} disabled={busy} autoFocus>
          {blocked ? 'Close' : 'Cancel'}
        </button>
        {prepared && !blocked && (
          <button type="button" className="ck-btn ck-btn--primary" disabled={busy || (!updating && !title.trim())} onClick={() => void submit()}>
            {busy ? 'Working…' : updating ? 'Update PR' : 'Create draft PR'}
          </button>
        )}
      </div>
    </Sheet>
  );
}
