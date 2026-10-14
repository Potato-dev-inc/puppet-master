// The UI copy of the broad-folder rule. The authority is Rust (`plain_mode/guard.rs`): it refuses
// the same folders whatever this file says. This copy exists to answer at once, and both read the
// same list of cases (`folder-cases.json`), so they cannot drift apart.
export type Verdict =
  | 'ok'
  | 'empty'
  | 'home'
  | 'desktop'
  | 'documents'
  | 'downloads'
  | 'onedrive'
  | 'drive_root'
  | 'above_home'
  | 'system'
  | 'projects_root';

export interface KnownDirs {
  home: string;
  onedrive: string[];
  projectsRoot: string;
  systemDrive: string;
}

/** Lower case, `/` separators, no trailing `/`, no `\\?\` prefix. */
export function normalize(path: string): string {
  let text = path.trim().replace(/\\/g, '/');
  if (text.startsWith('//?/')) text = text.slice(4);
  const trimmed = text.replace(/\/+$/, '');
  return (trimmed === '' && text.startsWith('/') ? '/' : trimmed).toLowerCase();
}

const isDriveRoot = (p: string) => p === '/' || /^[a-z]:$/.test(p);
const insideOrEqual = (p: string, base: string) => p === base || p.startsWith(`${base}/`);

export function folderVerdict(path: string, known: KnownDirs): Verdict {
  const p = normalize(path);
  if (p === '') return 'empty';
  if (isDriveRoot(p)) return 'drive_root';
  const drive = normalize(known.systemDrive);
  if (drive && ['windows', 'program files', 'program files (x86)', 'programdata'].some((n) => insideOrEqual(p, `${drive}/${n}`))) {
    return 'system';
  }
  const home = normalize(known.home);
  if (home) {
    if (insideOrEqual(p, `${home}/appdata`)) return 'system';
    if (home.startsWith(`${p}/`)) return 'above_home';
  }
  const bases: [string, boolean][] = [];
  if (home) bases.push([home, false]);
  for (const root of known.onedrive) if (normalize(root)) bases.push([normalize(root), true]);
  for (const [base, isOneDrive] of bases) {
    if (p === base) return isOneDrive ? 'onedrive' : 'home';
    if (p === `${base}/desktop`) return 'desktop';
    if (p === `${base}/documents`) return 'documents';
    if (p === `${base}/downloads`) return 'downloads';
  }
  if (known.projectsRoot && p === normalize(known.projectsRoot)) return 'projects_root';
  return 'ok';
}

/** The refusal code the Rust side uses for this verdict, or `null` when the folder is fine. */
export function refusalCode(verdict: Verdict): 'FOLDER_TOO_BROAD' | 'FOLDER_MISSING' | null {
  if (verdict === 'ok') return null;
  return verdict === 'empty' ? 'FOLDER_MISSING' : 'FOLDER_TOO_BROAD';
}
