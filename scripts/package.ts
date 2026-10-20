import { gzipSync } from 'node:zlib';
import { withMetadata } from './metadata.ts';
import { defaultWasmPath, Ruff } from './wasm.ts';

export async function packageWasm(path: string | URL): Promise<void> {
	const input = await Deno.readFile(path);
	const engine = new Ruff(await WebAssembly.compile(input));
	const metadata = engine.metadata();
	const metadataJSON = JSON.stringify(metadata, null, 2) + '\n';
	const wasm = withMetadata(input, new TextEncoder().encode(metadataJSON));
	await Deno.mkdir('artifacts', { recursive: true });
	await Deno.writeFile('artifacts/plugin.wasm', wasm);
	// Node's gzip encoder uses a zero timestamp, making the asset reproducible.
	const compressed = gzipSync(wasm, { level: 9 });
	await Deno.writeFile('artifacts/plugin.wasm.gz', compressed);
	await Deno.writeTextFile('artifacts/metadata.json', metadataJSON);
	await Deno.writeTextFile('artifacts/schema.json', JSON.stringify(metadata.config_schema, null, 2) + '\n');
	await Deno.copyFile('LICENSE', 'artifacts/LICENSE');
	await Deno.copyFile('LICENSE-RUFF', 'artifacts/LICENSE-RUFF');
	const hashes = [];
	for (const filename of ['plugin.wasm', 'plugin.wasm.gz', 'metadata.json', 'schema.json', 'LICENSE', 'LICENSE-RUFF']) {
		const digest = await crypto.subtle.digest('SHA-256', await Deno.readFile(`artifacts/${filename}`));
		const hex = Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, '0')).join('');
		hashes.push(`${hex}  ${filename}`);
	}
	await Deno.writeTextFile('artifacts/SHA256SUMS', hashes.join('\n') + '\n');
	console.log(
		`Packaged Ruff ${metadata.engine.version}: ${wasm.length} bytes raw, ${compressed.length} bytes gzip, schema extracted from plugin.wasm`,
	);
}

if (import.meta.main) await packageWasm(Deno.args[0] ?? defaultWasmPath);
