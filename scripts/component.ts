import { gzipSync } from 'node:zlib';
import { withComponentMetadata } from './metadata.ts';

/** Package the typed component separately, preserving the legacy core artifacts. */
export async function packageComponent(): Promise<void> {
	const command = new Deno.Command('cargo', {
		args: ['run', '--locked', '--package', 'actionlint-plugin-ruff-component', '--bin', 'component-metadata'],
		stdout: 'piped',
		stderr: 'inherit',
	});
	const output = await command.output();
	if (!output.success) throw new Error(`component metadata exited ${output.code}`);
	const encoder = await new Deno.Command('cargo', {
		args: [
			'run',
			'--locked',
			'--package',
			'actionlint-plugin-ruff-component',
			'--bin',
			'componentize',
			'--',
			'target/wasm32-unknown-unknown/release/actionlint_plugin_ruff_component.wasm',
		],
		stdout: 'piped',
		stderr: 'inherit',
	}).output();
	if (!encoder.success) throw new Error(`component encoder exited ${encoder.code}`);
	const component = withComponentMetadata(encoder.stdout, output.stdout);
	await Deno.writeFile('artifacts/plugin.component.wasm', component);
	await Deno.writeFile('artifacts/plugin.component.wasm.gz', gzipSync(component, { level: 9 }));
	await Deno.writeFile('artifacts/component.metadata.json', output.stdout);
	const hashes = [];
	for (const filename of ['plugin.component.wasm', 'plugin.component.wasm.gz', 'component.metadata.json']) {
		const digest = await crypto.subtle.digest('SHA-256', await Deno.readFile(`artifacts/${filename}`));
		const hex = Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, '0')).join('');
		hashes.push(`${hex}  ${filename}`);
	}
	await Deno.writeTextFile('artifacts/SHA256SUMS', hashes.join('\n') + '\n', { append: true });
	console.log(`Packaged typed Ruff component: ${component.length} bytes`);
}
