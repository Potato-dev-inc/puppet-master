/* tslint:disable */
/* eslint-disable */

export function authenticate(header: string, method: string, path_and_query: string, body: Uint8Array, now_ms: number): string;

/**
 * A `Batch` as JSON text (see `batch_core`); an empty string when `items` is not what the Worker stores.
 */
export function batch(items: string, next: number, head: number, reset: boolean): string;

export function check_acl(body: Uint8Array, team: string, root: string): string;

export function check_blob(body: Uint8Array, team: string, slot: string, root: string): string;

export function check_envelope(body: Uint8Array, team: string, device: string): string;

export function check_join(body: Uint8Array, team: string): string;

export function check_registration(body: Uint8Array): string;

export function constants(): string;

export function is_team_id(id: string): boolean;

/**
 * The WebSocket resync frame as JSON text.
 */
export function resync(head: number): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly authenticate: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number) => void;
    readonly batch: (a: number, b: number, c: number, d: number, e: number, f: number) => void;
    readonly check_acl: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => void;
    readonly check_blob: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number) => void;
    readonly check_envelope: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => void;
    readonly check_join: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly check_registration: (a: number, b: number, c: number) => void;
    readonly constants: (a: number) => void;
    readonly is_team_id: (a: number, b: number) => number;
    readonly resync: (a: number, b: number) => void;
    readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
    readonly __wbindgen_export: (a: number, b: number) => number;
    readonly __wbindgen_export2: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_export3: (a: number, b: number, c: number) => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
