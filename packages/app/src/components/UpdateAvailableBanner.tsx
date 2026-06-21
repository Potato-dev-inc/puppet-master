import type { AppUpdateState } from '../hooks/useAppUpdateCheck';

export function UpdateAvailableBanner({ update }: { update: AppUpdateState }) {
  const { result, dismissed, dismiss, openRelease, checking } = update;
  if (checking || dismissed || !result?.updateAvailable || !result.latestVersion) {
    return null;
  }

  return (
    <div
      className="pointer-events-none fixed inset-x-0 bottom-4 z-[80] flex justify-center px-4"
      role="status"
      aria-live="polite"
    >
      <div className="pointer-events-auto flex max-w-xl flex-wrap items-center gap-3 rounded-xl border border-pm-accent/40 bg-pm-panel px-4 py-3 shadow-lg shadow-black/30">
        <div className="min-w-0 flex-1 text-sm leading-6 text-pm-text">
          <span className="font-semibold">New version available</span>
          <span className="text-pm-muted">
            {' '}
            — v{result.latestVersion} is ready (you have v{result.currentVersion}).
          </span>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <button
            type="button"
            onClick={() => void openRelease()}
            className="rounded-lg bg-pm-accent px-3 py-1.5 text-sm font-semibold text-white transition hover:opacity-90"
          >
            Download & install
          </button>
          <button
            type="button"
            onClick={dismiss}
            className="rounded-lg border border-pm-border px-3 py-1.5 text-sm text-pm-muted transition hover:bg-pm-border/40"
            aria-label="Dismiss update notification"
          >
            Dismiss
          </button>
        </div>
      </div>
    </div>
  );
}
