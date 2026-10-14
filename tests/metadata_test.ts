import { deepStrictEqual as equal, throws } from 'node:assert/strict';
import { requireInterface, resolveInterface } from '../scripts/contract.ts';
import { MAX_METADATA_BYTES, readMetadata, readMetadataBytes, withMetadata } from '../scripts/metadata.ts';

const header = new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0]);
const fixture = {
	abi_version: 1,
	plugin_version: '0.1.0',
	engine: { name: 'ruff', version: '0.17.0' },
	capabilities: { check: true, noqa: false },
	config_schema: { type: 'object', properties: { lint: { type: 'object' } } },
};
const encode = (value: unknown) => new TextEncoder().encode(JSON.stringify(value) + '\n');

Deno.test('interface negotiation preserves v1 alongside newer and unknown capabilities', () => {
	const metadata = readMetadata(withMetadata(
		header,
		encode({
			...fixture,
			metadata_version: 1,
			future_field: { arbitrary: true },
			interfaces: {
				check: [{ version: 1, export: 'check' }, { version: 2, export: 'check_v2' }],
				format: [{ version: 2, export: 'format_v2' }],
				future_operation: [{ version: 7, export: 'future_v7' }],
			},
		}),
	));
	equal(requireInterface(metadata, 'check', 1), 'check');
	equal(resolveInterface(metadata, 'format', 1), undefined);
	throws(() => requireInterface(metadata, 'format', 1), /unsupported interface/);
	// Removing v1 is detected even when the operation itself remains available.
	throws(
		() => requireInterface({ ...metadata, interfaces: { check: [{ version: 2, export: 'check_v2' }] } }, 'check', 1),
		/unsupported interface/,
	);
	throws(() => requireInterface({ ...metadata, abi_version: 2 }, 'check', 1), /unsupported ABI/);
	throws(() => requireInterface({ ...metadata, metadata_version: 2 }, 'check', 1), /unsupported metadata/);
	throws(() => requireInterface({ ...metadata, transport: 'future-component' }, 'check', 1), /unsupported transport/);
});

Deno.test('legacy discovery falls back only to the original v1 interfaces', () => {
	const metadata = readMetadata(withMetadata(header, encode(fixture)));
	equal(requireInterface(metadata, 'check', 1), 'check');
	equal(resolveInterface(metadata, 'check', 2), undefined);
	equal(resolveInterface(metadata, 'format', 1), undefined);
	equal(resolveInterface(metadata, 'fix', 1), undefined);
	equal(resolveInterface(metadata, 'toString', 1), undefined);
	throws(() => withMetadata(header, encode({ ...fixture, metadata_version: 1 })), /invalid versioned/);
});

Deno.test('malformed discovery fields and ambiguous interface versions are rejected', () => {
	for (
		const extra of [
			{ languages: 'python' },
			{ shells: [42] },
			{ conflicts: [null] },
			{ homepage: false },
			{ interfaces: { check: true } },
			{ interfaces: { check: [{ version: 0, export: 'check' }] } },
			{ interfaces: { check: [{ version: 1, export: '' }] } },
			{ interfaces: { check: [{ version: 1, export: 'check', exclusive_group: false }] } },
			{ interfaces: { check: [{ version: 1, export: 'check' }, { version: 1, export: 'ambiguous' }] } },
		]
	) throws(() => withMetadata(header, encode({ ...fixture, ...extra })), /invalid metadata/);
});

Deno.test('metadata is readable from a prefix without executable code', () => {
	// This prefix deliberately ends before any types or functions: metadata needs no runtime.
	const prefix = withMetadata(header, encode(fixture));
	equal(prefix[8], 0);
	equal(readMetadata(prefix), fixture);
	equal(readMetadataBytes(prefix), encode(fixture));
	// Code is never validated or touched by metadata inspection.
	const invalidTail = new Uint8Array([...prefix, 10, 1, 255]);
	equal(readMetadata(invalidTail), fixture);
});

Deno.test('repackaging replaces metadata and preserves unrelated custom sections', () => {
	const other = new Uint8Array([...header, 0, 5, 1, 120, 97, 98, 99]); // custom section x: abc
	const first = withMetadata(other, encode(fixture));
	equal(withMetadata(first, encode(fixture)), first);
	const updated = { ...fixture, plugin_version: '0.2.0' };
	const second = withMetadata(first, encode(updated));
	equal(readMetadata(second), updated);
	equal(second.subarray(second.length - 7), other.subarray(8));
});

Deno.test('missing, truncated and overflowing sections are rejected', () => {
	throws(() => readMetadata(header), /missing/);
	throws(() => readMetadata(new Uint8Array([0, 97, 115, 109, 13, 0, 1, 0])), /header/);
	throws(() => readMetadata(new Uint8Array([...header, 0, 128])), /truncated/);
	throws(() => readMetadata(new Uint8Array([...header, 0, 255, 255, 255, 255, 16])), /u32/);
	throws(() => readMetadata(new Uint8Array([...header, 0, 1, 4])), /name exceeds/);
	const valid = withMetadata(header, encode(fixture));
	throws(() => readMetadata(valid.subarray(0, valid.length - 1)), /truncated/);
});

Deno.test('invalid JSON, UTF-8, metadata shapes and oversized payloads are rejected', () => {
	throws(() => withMetadata(header, new TextEncoder().encode('{')), SyntaxError);
	throws(() => withMetadata(header, new Uint8Array([255])), TypeError);
	for (const value of [null, [], {}, { ...fixture, abi_version: '1' }, { ...fixture, config_schema: [] }]) {
		throws(() => withMetadata(header, encode(value)), /invalid plugin metadata/);
	}
	throws(() => withMetadata(header, new Uint8Array(MAX_METADATA_BYTES + 1)), /exceeds/);
});

Deno.test('packaged metadata matches the sidecar byte for byte without instantiation', async () => {
	const wasm = await Deno.readFile(new URL('../artifacts/plugin.wasm', import.meta.url));
	const sidecar = await Deno.readFile(new URL('../artifacts/metadata.json', import.meta.url));
	const payload = readMetadataBytes(wasm);
	equal(payload, sidecar);
	const prefixLength = payload.byteOffset - wasm.byteOffset + payload.length;
	equal(readMetadata(wasm.subarray(0, prefixLength)), JSON.parse(new TextDecoder().decode(sidecar)));
	equal(readMetadata(wasm), JSON.parse(new TextDecoder().decode(sidecar)));
	const schema = JSON.parse(await Deno.readTextFile(new URL('../artifacts/schema.json', import.meta.url)));
	equal(readMetadata(wasm).config_schema, schema);
});
