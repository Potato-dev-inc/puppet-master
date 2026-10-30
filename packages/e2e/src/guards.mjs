// Safety rails: the founder's running app must never be touched by a test run.
import path from 'node:path';

export const FORBIDDEN_PORTS = [17321, 17322, 9333];

export function assertSafePort(port, what) {
  if (!Number.isInteger(port) || port <= 0 || port > 65535) throw new Error(`${what}: bad port ${port}`);
  if (FORBIDDEN_PORTS.includes(port)) throw new Error(`${what}: port ${port} belongs to the running app. Refusing.`);
}

/** True for the data folder of the real app (the default profile), which a test run must never use. */
export function isFoundersDataDir(dir) {
  const norm = path.resolve(dir).split(path.sep).join('/').toLowerCase();
  return /\/com\.puppetmaster\.app$/.test(norm);
}

export function assertSafeProfile(profile) {
  if (!/^[a-z0-9-]{1,32}$/.test(profile) || profile === 'default' || profile.startsWith('-')) {
    throw new Error(`bad profile name "${profile}" (the default profile is the real app)`);
  }
}
