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

export interface Response {
	ok: boolean;
	errors?: string[];
	diagnostics: Diagnostic[];
}

export interface Metadata {
	abi_version: number;
	engine: { name: string; version: string };
	capabilities: { noqa: boolean };
	config_schema: { additionalProperties: boolean; properties: Record<string, unknown> };
}

export class Ruff {
	readonly #exports: Exports;

	constructor(module: WebAssembly.Module) {
		const imports = WebAssembly.Module.imports(module);
		if (imports.length !== 0) throw new Error(`plugin must have no host imports: ${JSON.stringify(imports)}`);
		this.#exports = new WebAssembly.Instance(module, {}).exports as unknown as Exports;
		if (this.#exports.abi_version() !== 1) throw new Error('unsupported ABI version');
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

	check(source: string, config: unknown = {}, filename = 'workflow.step.py'): Response {
		return this.raw('check', JSON.stringify({ source, filename, config }));
	}

	validate(config: unknown): Response {
		return this.raw('validate_config', JSON.stringify(config));
	}

	metadata(): Metadata {
		return this.#response(this.#exports.metadata());
	}

	invalidInput(pointer: number, length: number): Response {
		return this.#response(this.#exports.check(pointer, length));
	}
}

export const defaultWasmPath = new URL(
	'../target/wasm32-unknown-unknown/release/actionlint_plugin_ruff.wasm',
	import.meta.url,
);
