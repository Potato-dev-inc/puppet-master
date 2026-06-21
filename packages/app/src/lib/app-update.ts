/** GitHub releases used for desktop update checks. */
export const RELEASE_REPO = 'Potato-dev-inc/puppet-master';
export const RELEASES_LATEST_URL = `https://github.com/${RELEASE_REPO}/releases/latest`;
export const RELEASES_API_URL = `https://api.github.com/repos/${RELEASE_REPO}/releases/latest`;

export interface UpdateCheckResult {
  currentVersion: string;
  latestVersion: string | null;
  updateAvailable: boolean;
  releaseUrl: string;
  releaseNotes: string | null;
  error: string | null;
}

export function normalizeVersion(version: string): string {
  return version.trim().replace(/^v/i, '');
}

export function parseVersionParts(version: string): number[] {
  return normalizeVersion(version)
    .split(/[.-]/)
    .map((part) => {
      const match = /^(\d+)/.exec(part);
      return match ? Number.parseInt(match[1], 10) : 0;
    });
}

/** True when `latest` is strictly newer than `current`. */
export function isVersionNewer(latest: string, current: string): boolean {
  const a = parseVersionParts(latest);
  const b = parseVersionParts(current);
  const length = Math.max(a.length, b.length);
  for (let index = 0; index < length; index += 1) {
    const left = a[index] ?? 0;
    const right = b[index] ?? 0;
    if (left !== right) return left > right;
  }
  return false;
}

interface GitHubRelease {
  tag_name?: string;
  html_url?: string;
  body?: string | null;
}

export async function checkForAppUpdate(currentVersion: string): Promise<UpdateCheckResult> {
  const base: UpdateCheckResult = {
    currentVersion,
    latestVersion: null,
    updateAvailable: false,
    releaseUrl: RELEASES_LATEST_URL,
    releaseNotes: null,
    error: null,
  };

  try {
    const response = await fetch(RELEASES_API_URL, {
      headers: { Accept: 'application/vnd.github+json' },
    });
    if (!response.ok) {
      return {
        ...base,
        error: `Update check failed (${response.status})`,
      };
    }
    const payload = (await response.json()) as GitHubRelease;
    const latestVersion = payload.tag_name ? normalizeVersion(payload.tag_name) : null;
    if (!latestVersion) {
      return { ...base, error: 'Release response did not include a version tag' };
    }
    return {
      currentVersion,
      latestVersion,
      updateAvailable: isVersionNewer(latestVersion, currentVersion),
      releaseUrl: payload.html_url?.trim() || RELEASES_LATEST_URL,
      releaseNotes: payload.body?.trim() || null,
      error: null,
    };
  } catch (error) {
    return {
      ...base,
      error: error instanceof Error ? error.message : String(error),
    };
  }
}
