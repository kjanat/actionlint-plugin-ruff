import type { Metadata } from './contract.ts';

export const METADATA_SECTION = 'actionlint.plugin';
export const MAX_METADATA_BYTES = 1024 * 1024;
const header = new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0]);
const encoder = new TextEncoder();
const decoder = new TextDecoder('utf-8', { fatal: true });

function checkHeader(bytes: Uint8Array): void {
	if (!header.every((value, i) => bytes[i] === value)) throw new Error('expected a core WASM v1 header');
}

function readU32(bytes: Uint8Array, offset: number, end = bytes.length): [number, number] {
	let value = 0;
	for (let i = 0; i < 5; i++) {
		if (offset >= end) throw new Error('truncated WASM length');
		const byte = bytes[offset++];
		if (i === 4 && (byte & 0xf0) !== 0) throw new Error('WASM length exceeds u32');
		value += (byte & 0x7f) * 2 ** (7 * i);
		if ((byte & 0x80) === 0) return [value, offset];
	}
	throw new Error('invalid WASM length');
}

function encodeU32(value: number): number[] {
	if (!Number.isSafeInteger(value) || value < 0 || value > 0xffffffff) throw new Error('WASM length exceeds u32');
	const bytes = [];
	do {
		const byte = value & 0x7f;
		value = Math.floor(value / 128);
		bytes.push(byte | (value === 0 ? 0 : 0x80));
	} while (value !== 0);
	return bytes;
}

function customName(bytes: Uint8Array, offset: number, end: number): [string, number] {
	const [length, start] = readU32(bytes, offset, end);
	const next = start + length;
	if (next > end) throw new Error('custom section name exceeds its section');
	return [decoder.decode(bytes.subarray(start, next)), next];
}

function decodeMetadata(bytes: Uint8Array): Metadata {
	if (bytes.length > MAX_METADATA_BYTES) throw new Error('plugin metadata exceeds 1 MiB');
	const value = JSON.parse(decoder.decode(bytes));
	const object = (input: unknown): input is Record<string, unknown> =>
		typeof input === 'object' && input !== null && !Array.isArray(input);
	const version = (input: unknown): boolean => Number.isSafeInteger(input) && (input as number) >= 1;
	if (
		!object(value) || !Number.isSafeInteger(value.abi_version) || (value.abi_version as number) < 1
		|| typeof value.plugin_version !== 'string' || value.plugin_version.length === 0
		|| !object(value.engine) || typeof value.engine.name !== 'string' || typeof value.engine.version !== 'string'
		|| !object(value.capabilities) || !object(value.config_schema)
	) {
		throw new Error('invalid plugin metadata object');
	}
	if (value.metadata_version !== undefined && (!version(value.metadata_version) || !object(value.interfaces))) {
		throw new Error('invalid versioned plugin metadata');
	}
	for (
		const key of [
			'id',
			'name',
			'description',
			'license',
			'homepage',
			'repository',
			'documentation_url',
			'update_url',
			'update_format',
			'transport',
		]
	) {
		if (value[key] !== undefined && (typeof value[key] !== 'string' || value[key].length === 0)) {
			throw new Error(`invalid metadata field: ${key}`);
		}
	}
	for (const key of ['languages', 'shells', 'file_extensions', 'conflicts']) {
		if (
			value[key] !== undefined
			&& (!Array.isArray(value[key])
				|| !value[key].every((entry: unknown) => typeof entry === 'string' && entry.length > 0))
		) {
			throw new Error(`invalid metadata field: ${key}`);
		}
	}
	if (value.interfaces !== undefined) {
		if (!object(value.interfaces)) throw new Error('invalid metadata interfaces');
		for (const entries of Object.values(value.interfaces)) {
			if (!Array.isArray(entries)) throw new Error('invalid metadata interface versions');
			const seen = new Set<number>();
			for (const entry of entries) {
				if (
					!object(entry) || !version(entry.version) || typeof entry.export !== 'string' || entry.export.length === 0
					|| (entry.exclusive_group !== undefined
						&& (typeof entry.exclusive_group !== 'string' || entry.exclusive_group.length === 0))
					|| seen.has(entry.version as number)
				) throw new Error('invalid metadata interface descriptor');
				seen.add(entry.version as number);
			}
		}
	}
	return value as unknown as Metadata;
}

/** Read only section bytes. A prefix ending after the metadata section is sufficient. */
export function readMetadataBytes(wasm: Uint8Array): Uint8Array {
	checkHeader(wasm);
	let offset = header.length;
	while (offset < wasm.length) {
		const id = wasm[offset++];
		const [size, start] = readU32(wasm, offset);
		const end = start + size;
		if (end > wasm.length) throw new Error('truncated WASM section');
		if (id === 0) {
			const [name, payload] = customName(wasm, start, end);
			if (name === METADATA_SECTION) {
				if (end - payload > MAX_METADATA_BYTES) throw new Error('plugin metadata exceeds 1 MiB');
				return wasm.subarray(payload, end);
			}
		}
		offset = end;
	}
	throw new Error(`missing ${METADATA_SECTION} custom section`);
}

/** Inspect metadata without compiling, instantiating, or executing guest code. */
export function readMetadata(wasm: Uint8Array): Metadata {
	return decodeMetadata(readMetadataBytes(wasm));
}

/** Put one metadata section first, replacing any earlier copy while preserving other sections. */
export function withMetadata(wasm: Uint8Array, metadata: Uint8Array): Uint8Array {
	checkHeader(wasm);
	decodeMetadata(metadata);
	const name = encoder.encode(METADATA_SECTION);
	const nameLength = encodeU32(name.length);
	const sectionHeader = new Uint8Array([
		0,
		...encodeU32(nameLength.length + name.length + metadata.length),
		...nameLength,
	]);
	const chunks = [header, sectionHeader, name, metadata];
	let offset = header.length;
	while (offset < wasm.length) {
		const sectionStart = offset;
		const id = wasm[offset++];
		const [size, start] = readU32(wasm, offset);
		const end = start + size;
		if (end > wasm.length) throw new Error('truncated WASM section');
		if (id !== 0 || customName(wasm, start, end)[0] !== METADATA_SECTION) chunks.push(wasm.subarray(sectionStart, end));
		offset = end;
	}
	const output = new Uint8Array(chunks.reduce((length, chunk) => length + chunk.length, 0));
	let position = 0;
	for (const chunk of chunks) {
		output.set(chunk, position);
		position += chunk.length;
	}
	return output;
}
