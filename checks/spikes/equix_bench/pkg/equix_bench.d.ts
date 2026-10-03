/* tslint:disable */
/* eslint-disable */

/**
 * Only the hashx program build for nonce `i` (1 if ok, 0 if skipped).
 */
export function build_one(i: number): number;

/**
 * Allocates the solver memory once (~1.8 MB), so solve timing excludes it.
 */
export function init(): void;

/**
 * One Equi-X attempt for nonce `i` (interpreted hashx: no JIT in wasm).
 * Returns the number of solutions, or -1 if the challenge is skipped.
 */
export function solve_one(i: number): number;

/**
 * Verifies every solution of the last solve (program build included per solution).
 * Returns how many verified.
 */
export function verify_last(): number;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly build_one: (a: number) => number;
    readonly init: () => void;
    readonly solve_one: (a: number) => number;
    readonly verify_last: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_start: () => void;
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
