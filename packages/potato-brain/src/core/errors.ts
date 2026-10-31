/**
 * Error codes of channel protocol v1. An error that leaves the brain carries
 * one of these and nothing else: never a response body, a header or a key.
 */
export const ERROR_CODES = [
  "PROVIDER_AUTH",
  "PROVIDER_LIMIT",
  "PROVIDER_DOWN",
  "NETWORK",
  "PERSONA_TOO_BIG",
  "SURFACE_AUTH",
  "INTERNAL",
] as const;

export type ErrorCode = (typeof ERROR_CODES)[number];

export class CodedError extends Error {
  readonly code: ErrorCode;
  readonly status: number | undefined;

  constructor(code: ErrorCode, message: string, status?: number) {
    super(message);
    this.name = "CodedError";
    this.code = code;
    this.status = status;
  }
}

/** Map an HTTP status of a model endpoint to a code. The body is never read. */
export function codeForStatus(status: number): ErrorCode {
  if (status === 401 || status === 403) return "PROVIDER_AUTH";
  // 402 is "out of balance" on the endpoints we talk to, 429 is a rate limit.
  if (status === 402 || status === 429) return "PROVIDER_LIMIT";
  if (status >= 500) return "PROVIDER_DOWN";
  return "INTERNAL";
}

export function isCodedError(err: unknown): err is CodedError {
  return err instanceof CodedError;
}

/** Anything thrown becomes one code. Unknown errors are INTERNAL. */
export function codeOf(err: unknown): ErrorCode {
  return isCodedError(err) ? err.code : "INTERNAL";
}
