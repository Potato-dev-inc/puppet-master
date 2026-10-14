// The Settings window: six sections, row search, and "Coming later". macOS System Settings style.
//
// Nothing here has a Save button. Every control saves itself through `env.patch()`, which sends only the
// keys that control changed (`patchSettings` -> the Rust `settings_patch` command), so this window can
// never write an old copy of the settings over newer values. The window reloads its state whenever the
// settings change anywhere else, and whenever it is shown again (it is hidden, not destroyed, on close).
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { Settings } from '@puppet-master/shared';
import { findBridgeUrl } from '../lib/bridge';
import { loadSettings, patchSettings, type SettingsPatch } from '../lib/settings';
import { subscribeSettingsRefresh, type SettingsRefreshInfo } from '../lib/settings-sync';
import type { UpdateCheckResult } from '../lib/app-update';
import { resolveSettingsTab } from '../lib/settings-entry';
import { LATER_ITEMS, SECTIONS } from './settings/catalog';
import { ConfirmSheet } from './settings/controls';
import { SectionRows, type RowChrome } from './settings/SectionView';
import {
  searchSections,
  type ConfirmOptions,
  type RowDef,
  type SettingsEnv,
  type SettingsPageId,
} from './settings/model';
import '../styles/settings.css';

/** What the panel needs from the outside. Tests pass fakes; the app uses the defaults. */
export interface SettingsApi {
  load: () => Promise<Settings>;
  patch: (patch: SettingsPatch) => Promise<Settings>;
  findBridgeUrl: () => Promise<string | null>;
  subscribe: (onRefresh: (info: SettingsRefreshInfo) => void) => () => void;
}

const DEFAULT_API: SettingsApi = {
  load: loadSettings,
  patch: patchSettings,
  findBridgeUrl,
  subscribe: subscribeSettingsRefresh,
};

interface Props {
  open: boolean;
  onClose?: () => void;
  /** Page shown first (the standalone Settings window opens on Agents). */
  initialTab?: SettingsPageId;
  /** Called with the page now shown, so a host can carry it to another surface (window <-> inline). */
  onPageChange?: (page: SettingsPageId) => void;
  /** Rendered inside the cockpit's main area (Orchestration) instead of filling a window. */
  inline?: boolean;
  /** Called after any control saved. */
  onSaved?: () => void;
  projectPath?: string | null;
  onProjectPathChange?: (path: string) => Promise<void>;
  updateCheck?: UpdateCheckResult | null;
  updateChecking?: boolean;
  onCheckForUpdates?: () => void;
  onOpenRelease?: () => void;
  api?: SettingsApi;
}

const SAVED_TICK_MS = 1600;

function withoutNulls(patch: SettingsPatch): Partial<Settings> {
  const out: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(patch)) if (value !== null && value !== undefined) out[key] = value;
  return out as Partial<Settings>;
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

const IconBack = () => (
  <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <path d="M10 3.5 5.5 8l4.5 4.5" />
  </svg>
);
const IconSearch = () => (
  <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <circle cx="7" cy="7" r="4.5" />
    <path d="m10.5 10.5 3 3" />
  </svg>
);
const IconLater = () => (
  <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <circle cx="8" cy="8" r="5.5" />
    <path d="M8 5v3.2l2 1.3" />
  </svg>
);

export function SettingsPanel({
  open,
  onClose,
  initialTab = 'general',
  onPageChange,
  inline = false,
  onSaved,
  projectPath = null,
  onProjectPathChange,
  updateCheck,
  updateChecking,
  onCheckForUpdates,
  onOpenRelease,
  api = DEFAULT_API,
}: Props) {
  const [page, setPage] = useState<SettingsPageId>(initialTab);
  const [query, setQuery] = useState('');
  const [settings, setSettings] = useState<Settings | null>(null);
  const [bridgeUrl, setBridgeUrl] = useState<string | null>(null);
  const [savedIds, setSavedIds] = useState<ReadonlySet<string>>(new Set());
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [pendingConfirm, setPendingConfirm] = useState<{ options: ConfirmOptions; resolve: (ok: boolean) => void } | null>(null);

  const inflight = useRef(0);
  const reloadAfter = useRef(false);
  const tickTimers = useRef(new Map<string, ReturnType<typeof setTimeout>>());
  const scrollRef = useRef<HTMLDivElement>(null);
  const apiRef = useRef(api);
  apiRef.current = api;

  const reload = useCallback(async () => {
    if (inflight.current > 0) {
      reloadAfter.current = true; // a save is still going out: reload once it lands
      return;
    }
    try {
      const fresh = await apiRef.current.load();
      if (inflight.current === 0) setSettings(fresh);
    } catch {
      /* keep what is shown */
    }
  }, []);

  // Load on open, and again whenever the settings may have changed elsewhere or the window comes back.
  useEffect(() => {
    if (!open) return;
    void reload();
    void apiRef.current.findBridgeUrl().then(setBridgeUrl).catch(() => undefined);
    return apiRef.current.subscribe((info) => {
      void reload();
      if (info.reason === 'show') {
        const target = resolveSettingsTab(info.tab);
        if (target) {
          setPage(target);
          setQuery('');
        }
      }
    });
  }, [open, reload]);

  useEffect(() => {
    scrollRef.current?.scrollTo?.({ top: 0 });
  }, [page, query]);

  const pageChangeRef = useRef(onPageChange);
  pageChangeRef.current = onPageChange;
  useEffect(() => {
    pageChangeRef.current?.(page);
  }, [page]);

  useEffect(() => {
    const timers = tickTimers.current;
    return () => timers.forEach((t) => clearTimeout(t));
  }, []);

  const markSaved = useCallback((rowId: string) => {
    setSavedIds((prev) => new Set(prev).add(rowId));
    const timers = tickTimers.current;
    const existing = timers.get(rowId);
    if (existing) clearTimeout(existing);
    timers.set(
      rowId,
      setTimeout(() => {
        timers.delete(rowId);
        setSavedIds((prev) => {
          const next = new Set(prev);
          next.delete(rowId);
          return next;
        });
      }, SAVED_TICK_MS),
    );
  }, []);

  const setRowError = useCallback((rowId: string, message: string | null) => {
    setErrors((prev) => {
      if (message === null) {
        if (!(rowId in prev)) return prev;
        const { [rowId]: _gone, ...rest } = prev;
        return rest;
      }
      return { ...prev, [rowId]: message };
    });
  }, []);

  const patchFor = useCallback(
    async (rowId: string, patch: SettingsPatch) => {
      setRowError(rowId, null);
      setSettings((prev) => (prev ? { ...prev, ...withoutNulls(patch) } : prev));
      inflight.current += 1;
      try {
        await apiRef.current.patch(patch);
        markSaved(rowId);
        onSaved?.();
      } catch (error) {
        setRowError(rowId, `Not saved: ${errorText(error)}`);
        reloadAfter.current = true; // drop the optimistic value and show what is really saved
      } finally {
        inflight.current -= 1;
        if (inflight.current === 0 && reloadAfter.current) {
          reloadAfter.current = false;
          void reload();
        }
      }
    },
    [markSaved, onSaved, reload, setRowError],
  );

  const confirm = useCallback(
    (options: ConfirmOptions) => new Promise<boolean>((resolve) => setPendingConfirm({ options, resolve })),
    [],
  );
  const settle = (ok: boolean) => {
    pendingConfirm?.resolve(ok);
    setPendingConfirm(null);
  };

  const update = useMemo(
    () => ({ result: updateCheck, checking: updateChecking, onCheck: onCheckForUpdates, onOpenRelease }),
    [updateCheck, updateChecking, onCheckForUpdates, onOpenRelease],
  );

  const envFor = (row: RowDef): SettingsEnv => ({
    settings: settings as Settings,
    patch: (patch) => patchFor(row.id, patch),
    markSaved: () => markSaved(row.id),
    setError: (message) => setRowError(row.id, message),
    confirm,
    projectPath,
    onProjectPathChange,
    bridgeUrl,
    update,
  });

  if (!open) return null;

  const trimmed = query.trim();
  const hits = trimmed ? searchSections(SECTIONS, trimmed) : [];
  const section = SECTIONS.find((s) => s.id === page);
  const chrome: RowChrome = { saved: savedIds, errors };

  const title = trimmed ? 'Search results' : page === 'later' ? 'Coming later' : (section?.title ?? 'Settings');
  const blurb = trimmed
    ? undefined
    : page === 'later'
      ? 'Not built yet. None of these do anything today.'
      : section?.blurb;

  return (
    <div className={inline ? 'settings settings--inline' : 'settings'} role="region" aria-label="Settings">
      <aside className="st-side">
        <div className="st-side-top">
          {onClose && (
            <button type="button" className="st-back" aria-label={inline ? 'Back' : 'Close settings'} onClick={onClose}>
              <IconBack />
            </button>
          )}
          <label className="st-search">
            <IconSearch />
            <input
              type="search"
              aria-label="Search settings"
              placeholder="Search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Escape') setQuery('');
              }}
            />
          </label>
        </div>
        <nav className="st-nav" aria-label="Settings sections">
          {SECTIONS.map((item) => (
            <button
              key={item.id}
              type="button"
              className="st-nav-row"
              aria-current={!trimmed && page === item.id ? 'page' : undefined}
              onClick={() => {
                setPage(item.id);
                setQuery('');
              }}
            >
              {item.title}
            </button>
          ))}
        </nav>
        <div className="st-side-foot">
          <button
            type="button"
            className="st-nav-row"
            aria-current={!trimmed && page === 'later' ? 'page' : undefined}
            onClick={() => {
              setPage('later');
              setQuery('');
            }}
          >
            <IconLater />
            <span>Coming later</span>
          </button>
        </div>
      </aside>

      <main className="st-main">
        <header className="st-title">
          <h1>{title}</h1>
          {blurb && <p>{blurb}</p>}
        </header>
        <div className="st-scroll" ref={scrollRef}>
          <div className="st-page">
            {trimmed ? (
              settings === null ? (
                <Skeleton />
              ) : hits.length === 0 ? (
                <p className="st-empty">No settings match “{trimmed}”.</p>
              ) : (
                <div className="st-results">
                  {hits.map(({ section: hitSection, rows }) => (
                    <section key={hitSection.id} data-section={hitSection.id}>
                      <h2>{hitSection.title}</h2>
                      <SectionRows rows={rows.map((r) => ({ ...r, group: undefined }))} envFor={envFor} chrome={chrome} />
                    </section>
                  ))}
                </div>
              )
            ) : page === 'later' ? (
              <section className="st-group">
                <div className="st-list">
                  {LATER_ITEMS.map((item) => (
                    <div key={item.title} className="st-rowwrap">
                      <div className="st-row">
                        <div className="st-label">
                          <span>{item.title}</span>
                          <small>{item.note}</small>
                        </div>
                        <div className="st-control">
                          <span className="st-value">Not available yet</span>
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              </section>
            ) : settings === null ? (
              <Skeleton />
            ) : (
              section && <SectionRows key={section.id} rows={section.rows} envFor={envFor} chrome={chrome} />
            )}
          </div>
        </div>
      </main>

      {pendingConfirm && (
        <ConfirmSheet
          title={pendingConfirm.options.title}
          body={pendingConfirm.options.body}
          confirmLabel={pendingConfirm.options.confirmLabel}
          onConfirm={() => settle(true)}
          onCancel={() => settle(false)}
        />
      )}
    </div>
  );
}

function Skeleton() {
  return (
    <div className="st-list" aria-busy="true" aria-label="Loading settings">
      {[0, 1, 2].map((n) => (
        <div key={n} className="st-skel">
          <span style={{ width: 120 + n * 28 }} />
          <span style={{ width: 52 }} />
        </div>
      ))}
    </div>
  );
}
