export type PuppetPlatform = 'windows' | 'macos' | 'linux';

export function detectPlatform(): PuppetPlatform {
  if (typeof process !== 'undefined' && process.platform) {
    switch (process.platform) {
      case 'win32':
        return 'windows';
      case 'darwin':
        return 'macos';
      default:
        return 'linux';
    }
  }

  if (typeof navigator !== 'undefined' && navigator.userAgent) {
    const ua = navigator.userAgent;
    if (/Windows/i.test(ua)) return 'windows';
    if (/Macintosh|Mac OS X/i.test(ua)) return 'macos';
  }

  return 'linux';
}

export function isWindows(platform: PuppetPlatform = detectPlatform()): boolean {
  return platform === 'windows';
}

export function isMacOS(platform: PuppetPlatform = detectPlatform()): boolean {
  return platform === 'macos';
}

export function isUnix(platform: PuppetPlatform = detectPlatform()): boolean {
  return platform === 'macos' || platform === 'linux';
}
