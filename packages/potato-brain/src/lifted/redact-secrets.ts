// Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7, file packages/core/src/events.ts (redactSecrets and its matching helpers).
// Copyright 2026 Rakazo contributors. Licensed under the Apache License, Version 2.0;
// see packages/potato-sandbox/LICENSE-APACHE-2.0. Changed by Puppet Master: redactSecrets also hides the base64 (standard
// and url-safe) and URL-encoded forms of each secret; the plain-value matching is Rakazo's, unchanged. Only redactSecrets is kept.

/** Shorter lines, or lines with spaces such as a key's BEGIN header, also appear in ordinary text. */
const MIN_SECRET_LINE_LENGTH = 16;

/**
 * The values to hide for `secrets`: each value, and each long unspaced line of a multi-line value
 * such as a private key's body, since output that prints only some lines or joins them with
 * spaces (an unquoted `echo $KEY`) never contains the whole value.
 */
function protectedValues(secrets: Iterable<string>): string[] {
  const values = new Set<string>();
  for (const secret of secrets) {
    if (!secret) continue;
    values.add(secret);
    if (!/[\r\n]/.test(secret)) continue;
    for (const line of secret.split(/\r\n?|\n/)) {
      const trimmed = line.trim();
      if (trimmed.length >= MIN_SECRET_LINE_LENGTH && !/\s/.test(trimmed)) values.add(trimmed);
    }
  }
  return [...values];
}

/**
 * The encoded spellings of one value that output can carry it in: standard and url-safe base64
 * (a `base64` pipe, a JSON blob), and percent-encoding in its usual and lower-case forms, with
 * spaces as `+` too (form encoding). A value that cannot be percent-encoded (a lone surrogate)
 * has no such form.
 */
function encodedForms(value: string): string[] {
  const forms = [
    Buffer.from(value, "utf8").toString("base64"),
    Buffer.from(value, "utf8").toString("base64url"),
  ];
  try {
    const percent = encodeURIComponent(value);
    forms.push(
      percent,
      percent.replace(/%20/g, "+"),
      percent.replace(/%[0-9A-F]{2}/g, (escape) => escape.toLowerCase()),
    );
  } catch {
    // No percent form for this value.
  }
  return forms;
}

/** Every spelling of every protected value: the plain value, then its encoded forms. */
function matchValues(secrets: Iterable<string>): string[] {
  const values = new Set<string>();
  for (const value of protectedValues(secrets)) {
    values.add(value);
    for (const form of encodedForms(value)) {
      if (form) values.add(form);
    }
  }
  return [...values];
}

/**
 * The stretches protected values cover in `value`, in order, as [start, end). Matches that
 * overlap make one stretch, so a shorter value inside or across a longer one can't cut it and
 * leave the rest readable; matches that only touch stay separate.
 */
function redactedRanges(value: string, values: Iterable<string>): Array<[number, number]> {
  // Count matches starting and ending at each offset rather than listing every match, so a value
  // that repeats or overlaps itself across a long text costs no more than two counters per offset.
  let starts: Int32Array | undefined;
  let ends: Int32Array | undefined;
  for (const secret of values) {
    for (let at = value.indexOf(secret); at !== -1; at = value.indexOf(secret, at + 1)) {
      starts ??= new Int32Array(value.length + 1);
      ends ??= new Int32Array(value.length + 1);
      starts[at] = starts[at]! + 1;
      ends[at + secret.length] = ends[at + secret.length]! + 1;
    }
  }
  if (!starts || !ends) return [];
  const ranges: Array<[number, number]> = [];
  let open = 0;
  let start = 0;
  for (let offset = 0; offset <= value.length; offset += 1) {
    const ending = ends[offset]!;
    const starting = starts[offset]!;
    if (ending === 0 && starting === 0) continue;
    open -= ending;
    if (ending > 0 && open === 0) ranges.push([start, offset]);
    if (starting > 0 && open === 0) start = offset;
    open += starting;
  }
  return ranges;
}

/**
 * Replace each secret, in plain, base64 (standard and url-safe) and URL-encoded form, with
 * `[redacted]`. Overlapping matches become one placeholder.
 */
export function redactSecrets(value: string, secrets: readonly string[]): string {
  let output = "";
  let offset = 0;
  for (const [start, end] of redactedRanges(value, matchValues(secrets))) {
    output += `${value.slice(offset, start)}[redacted]`;
    offset = end;
  }
  return output + value.slice(offset);
}
