import { requireInterface } from './contract.ts';
import type { Metadata } from './contract.ts';
export type { Metadata } from './contract.ts';

interface Exports {
	memory: WebAssembly.Memory;
	abi_version: () => number;
	alloc: (length: number) => number;
	dealloc: (pointer: number) => void;
	check: (pointer: number, length: number) => bigint;
	validate_config: (pointer: number, length: number) => bigint;
	metadata: () => bigint;
}

export interface Diagnostic {
	filename: string;
	code: string;
	name: string;
	severity: string;
	message: string;
	location: { row: number; column: number };
	end_location: { row: number; column: number };
	url: string | null;
}

export interface Failure {
	ok: false;
	errors: string[];
}

export interface CheckSuccess {
	ok: true;
	diagnostics: Diagnostic[];
}

export type CheckResponse = CheckSuccess | Failure;
export type ConfigResponse = { ok: true; errors: string[] } | Failure;
export type Response = CheckResponse | ConfigResponse;

export class Ruff {
	readonly #exports: Exports;

	constructor(module: WebAssembly.Module) {
		const imports = WebAssembly.Module.imports(module);
		if (imports.length !== 0) throw new Error(`plugin must have no host imports: ${JSON.stringify(imports)}`);
		this.#exports = new WebAssembly.Instance(module, {}).exports as unknown as Exports;
		if (this.#exports.abi_version() !== 1) throw new Error('unsupported ABI version');
		const metadata = this.metadata();
		for (const name of ['check', 'validate_config', 'metadata'] as const) {
			const entry = requireInterface(metadata, name, 1);
			if (entry !== name || typeof this.#exports[entry] !== 'function') {
				throw new Error(`unsupported ${name} v1 export: ${entry}`);
			}
		}
	}

	get memoryBytes(): number {
		return this.#exports.memory.buffer.byteLength;
	}

	#response<T>(result: bigint): T {
		const pointer = Number(result >> 32n);
		const length = Number(result & 0xffffffffn);
		try {
			// Calls can grow memory; obtain a fresh view after every call.
			const bytes = new Uint8Array(this.#exports.memory.buffer, pointer, length);
			return JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
		} finally {
			this.#exports.dealloc(pointer);
		}
	}

	raw(operation: 'check', input: string): CheckResponse;
	raw(operation: 'validate_config', input: string): ConfigResponse;
	raw(operation: 'check' | 'validate_config', input: string): Response {
		const bytes = new TextEncoder().encode(input);
		const pointer = this.#exports.alloc(bytes.length);
		if (pointer === 0) throw new Error('input allocation rejected');
		try {
			new Uint8Array(this.#exports.memory.buffer, pointer, bytes.length).set(bytes);
			return this.#response(this.#exports[operation](pointer, bytes.length));
		} finally {
			this.#exports.dealloc(pointer);
		}
	}

	check(source: string, config: unknown = {}, filename = 'workflow.step.py'): CheckResponse {
		return this.raw('check', JSON.stringify({ source, filename, config }));
	}

	validate(config: unknown): ConfigResponse {
		return this.raw('validate_config', JSON.stringify(config));
	}

	metadata(): Metadata {
		return this.#response(this.#exports.metadata());
	}

	invalidInput(pointer: number, length: number): CheckResponse {
		return this.#response(this.#exports.check(pointer, length));
	}
}

export const defaultWasmPath = new URL(
	'../target/wasm32-unknown-unknown/release/actionlint_plugin_ruff.wasm',
	import.meta.url,
);
