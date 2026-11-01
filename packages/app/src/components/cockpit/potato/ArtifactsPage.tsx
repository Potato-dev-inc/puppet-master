// The Artifacts page: the files the bots made or received. It takes the thread's place while it is open ("Back to chat"
// gives it back). Filters: the bot, the kind and part of the name, behind a Filters button. A list or a grid (remembered
// on this computer), and the preview of the chosen file. The list is read again each time a filter changes or a file is
// deleted.
import { useEffect, useMemo, useState } from 'react';
import { formatWhen, plainError } from '../../../lib/potato';
import {
  ARTIFACT_COPY,
  KIND_LABEL,
  KIND_ORDER,
  cleanFilter,
  type Artifact,
  type ArtifactKind,
  type ArtifactListItem,
  type ArtifactPreview as Preview,
  type ArtifactTransport,
} from '../../../lib/potato-artifacts';
import { ArtifactPreview } from './ArtifactPreview';
import '../../../styles/cockpit-potato-artifacts.css';

export interface ArtifactsPageProps {
  bots: { id: string; name: string }[];
  transport: ArtifactTransport;
  /** "Back to chat". */
  onClose: () => void;
  onNotify: (text: string) => void;
}

type View = 'list' | 'grid';
const VIEW_KEY = 'pm.potato.artifacts.view';

/** The view the person picked last time. A blocked store means the default (list). */
function readView(): View {
  try {
    return globalThis.localStorage?.getItem(VIEW_KEY) === 'grid' ? 'grid' : 'list';
  } catch {
    return 'list';
  }
}

function writeView(view: View): void {
  try {
    globalThis.localStorage?.setItem(VIEW_KEY, view);
  } catch {
    // Not remembered in this window: fine.
  }
}

/** One name of one bot: the key of a family of versions. */
const familyKey = (bot: string, name: string) => `${bot}\u0000${name.toLowerCase()}`;

export function ArtifactsPage({ bots, transport, onClose, onNotify }: ArtifactsPageProps) {
  const [view, setView] = useState<View>(readView);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [bot, setBot] = useState<string | undefined>(undefined);
  const [kind, setKind] = useState<ArtifactKind | undefined>(undefined);
  const [search, setSearch] = useState('');
  const [rows, setRows] = useState<ArtifactListItem[] | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const [reloads, setReloads] = useState(0);
  // The family on show (its newest version is its row), and a version picked from its Version select (null: the newest).
  const [active, setActive] = useState<string | null>(null);
  const [versionId, setVersionId] = useState<string | null>(null);
  const [preview, setPreview] = useState<Preview | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);

  const now = Date.now();
  const names = useMemo(() => new Map(bots.map((b) => [b.id, b.name])), [bots]);
  const botName = (id: string) => names.get(id) ?? id;
  const filter = useMemo(() => cleanFilter({ bot, kind, search }), [bot, kind, search]);
  const filtered = Boolean(filter.bot || filter.kind || filter.search);

  useEffect(() => {
    let alive = true;
    setListError(null);
    transport.list(filter).then(
      (found) => {
        if (alive) setRows(found);
      },
      (error) => {
        if (alive) {
          setRows([]);
          setListError(plainError(error, 'Could not read the artifacts.'));
        }
      },
    );
    return () => {
      alive = false;
    };
  }, [transport, filter, reloads]);

  // Keep the family on show while it is in the list; otherwise open the first one.
  useEffect(() => {
    if (!rows) return;
    if (active && rows.some((r) => familyKey(r.bot, r.name) === active)) return;
    setActive(rows.length ? familyKey(rows[0].bot, rows[0].name) : null);
    setVersionId(null);
  }, [rows, active]);

  const row = rows?.find((r) => familyKey(r.bot, r.name) === active) ?? null;
  const openBot = row?.bot ?? null;
  const openId = row ? (versionId ?? row.id) : null;

  useEffect(() => {
    if (!openBot || !openId) {
      setPreview(null);
      return;
    }
    let alive = true;
    setPreviewError(null);
    transport.open(openBot, openId).then(
      (found) => {
        if (alive) setPreview(found);
      },
      (error) => {
        if (alive) {
          setPreview(null);
          setPreviewError(plainError(error, 'Could not open the file.'));
        }
      },
    );
    return () => {
      alive = false;
    };
  }, [transport, openBot, openId]);

  const choose = (r: ArtifactListItem) => {
    setActive(familyKey(r.bot, r.name));
    setVersionId(null);
  };

  const download = async (a: Artifact) => {
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const dest = await save({ defaultPath: a.name });
      if (!dest) return;
      await transport.download(a.bot, a.id, dest);
      onNotify(ARTIFACT_COPY.downloaded(a.name));
    } catch (error) {
      onNotify(plainError(error, 'Could not download the file.'));
    }
  };

  const reveal = async (a: Artifact) => {
    try {
      await transport.reveal(a.bot, a.id);
    } catch (error) {
      onNotify(plainError(error, 'Could not open the folder.'));
    }
  };

  const remove = async (a: Artifact) => {
    try {
      await transport.remove(a.bot, a.id);
      setVersionId((cur) => (cur === a.id ? null : cur));
      setReloads((n) => n + 1);
      onNotify(ARTIFACT_COPY.deleted(a.name));
    } catch (error) {
      onNotify(plainError(error, 'Could not delete the file.'));
    }
  };

  const toggleView = () => {
    const next: View = view === 'list' ? 'grid' : 'list';
    setView(next);
    writeView(next);
  };

  return (
    <section className="ck-artifacts" aria-label={ARTIFACT_COPY.title}>
      <header className="ck-artifacts-head">
        <h2 className="ck-artifacts-title">{ARTIFACT_COPY.title}</h2>
        <div className="ck-artifacts-tools">
          <button type="button" className="ck-btn ck-btn--secondary" aria-pressed={filtersOpen} onClick={() => setFiltersOpen((v) => !v)}>
            {ARTIFACT_COPY.filters}
          </button>
          <button
            type="button"
            className="ck-icon-btn"
            aria-label={view === 'list' ? ARTIFACT_COPY.asGrid : ARTIFACT_COPY.asList}
            title={view === 'list' ? ARTIFACT_COPY.asGrid : ARTIFACT_COPY.asList}
            onClick={toggleView}
          >
            {view === 'list' ? <GridIcon /> : <ListIcon />}
          </button>
          <button type="button" className="ck-btn ck-btn--secondary" onClick={onClose}>
            {ARTIFACT_COPY.back}
          </button>
        </div>
      </header>

      {filtersOpen && (
        <div className="ck-artifacts-filters">
          <input
            type="search"
            className="ck-artifacts-search"
            aria-label={ARTIFACT_COPY.search}
            placeholder={ARTIFACT_COPY.search}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
          <div className="ck-artifacts-chips" role="group" aria-label="Bot">
            <Chip pressed={!bot} onClick={() => setBot(undefined)}>
              {ARTIFACT_COPY.allBots}
            </Chip>
            {bots.map((b) => (
              <Chip key={b.id} pressed={bot === b.id} onClick={() => setBot(b.id)}>
                {b.name}
              </Chip>
            ))}
          </div>
          <div className="ck-artifacts-chips" role="group" aria-label="Kind">
            <Chip pressed={!kind} onClick={() => setKind(undefined)}>
              {ARTIFACT_COPY.allKinds}
            </Chip>
            {KIND_ORDER.map((k) => (
              <Chip key={k} pressed={kind === k} onClick={() => setKind(k)}>
                {KIND_LABEL[k]}
              </Chip>
            ))}
          </div>
        </div>
      )}

      <div className="ck-artifacts-body">
        <div className={`ck-artifacts-list is-${view}`} aria-label="Files">
          {rows === null && <p className="ck-sub ck-artifacts-note">{ARTIFACT_COPY.loading}</p>}
          {listError && (
            <p role="alert" className="ck-artifacts-note">
              {listError}
            </p>
          )}
          {rows !== null && rows.length === 0 && !listError && <p className="ck-sub ck-artifacts-note">{filtered ? ARTIFACT_COPY.emptyFiltered : ARTIFACT_COPY.empty}</p>}
          {rows?.map((r) => {
            const on = familyKey(r.bot, r.name) === active;
            return (
              <button
                key={`${r.bot}-${r.id}`}
                type="button"
                className={`ck-artifact-row${on ? ' is-active' : ''}`}
                aria-current={on ? 'true' : undefined}
                onClick={() => choose(r)}
              >
                <span className="ck-artifact-badge">{KIND_LABEL[r.kind]}</span>
                <span className="ck-artifact-row-main">
                  <span className="ck-artifact-row-name">{r.name}</span>
                  <span className="ck-sub ck-artifact-row-meta">
                    {`${botName(r.bot)} · ${formatWhen(r.created, now)}${r.versions > 1 ? ` · ${r.versions} versions` : ''}`}
                  </span>
                </span>
              </button>
            );
          })}
        </div>

        <div className="ck-artifacts-preview">
          {previewError && (
            <p role="alert" className="ck-artifacts-note">
              {previewError}
            </p>
          )}
          {!preview && !previewError && <p className="ck-sub ck-artifacts-note">{rows && rows.length === 0 ? '' : ARTIFACT_COPY.pick}</p>}
          <ArtifactPreview
            preview={preview}
            botName={botName}
            now={now}
            onOpenVersion={(id) => setVersionId(row && id !== row.id ? id : null)}
            onDownload={(a) => void download(a)}
            onReveal={(a) => void reveal(a)}
            onRemove={(a) => void remove(a)}
          />
        </div>
      </div>
    </section>
  );
}

function Chip({ pressed, onClick, children }: { pressed: boolean; onClick: () => void; children: string }) {
  return (
    <button type="button" className="ck-artifacts-chip" aria-pressed={pressed} onClick={onClick}>
      {children}
    </button>
  );
}

function GridIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" aria-hidden="true">
      <rect x="2.5" y="2.5" width="4.5" height="4.5" rx="1" />
      <rect x="9" y="2.5" width="4.5" height="4.5" rx="1" />
      <rect x="2.5" y="9" width="4.5" height="4.5" rx="1" />
      <rect x="9" y="9" width="4.5" height="4.5" rx="1" />
    </svg>
  );
}

function ListIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" aria-hidden="true">
      <path d="M5.5 4h8M5.5 8h8M5.5 12h8M2.5 4h.01M2.5 8h.01M2.5 12h.01" />
    </svg>
  );
}
