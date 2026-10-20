import { packageComponent } from './component.ts';
import { packageWasm } from './package.ts';
import { defaultWasmPath } from './wasm.ts';

async function run(command: string, args: string[]): Promise<void> {
	const status = await new Deno.Command(command, { args, stdout: 'inherit', stderr: 'inherit' }).spawn().status;
	if (!status.success) throw new Error(`${command} exited ${status.code}`);
}

await run('cargo', ['build', '--locked', '--release', '--lib', '--target', 'wasm32-unknown-unknown']);
await Deno.mkdir('artifacts', { recursive: true });
const temporary = await Deno.makeTempFile({ dir: 'artifacts', suffix: '.wasm' });
try {
	await Deno.copyFile(defaultWasmPath, temporary);
	await run('wasm-opt', [
		temporary,
		'-o',
		'artifacts/plugin.wasm',
		// Restrict optimization to core features supported by Go hosts.
		// --all-features permits Binaryen to introduce typed references and GC.
		'--mvp-features',
		'--enable-bulk-memory',
		'--enable-multivalue',
		'--enable-mutable-globals',
		'--enable-nontrapping-float-to-int',
		'--enable-reference-types',
		'--enable-sign-ext',
		'--remove-unused-module-elements',
		'-Oz',
		'--strip-debug',
		'--strip-producers',
	]);
} finally {
	await Deno.remove(temporary);
}
// Derive metadata and compressed bytes from the final, import-free module.
await packageWasm('artifacts/plugin.wasm');
await run('cargo', [
	'build',
	'--locked',
	'--release',
	'--lib',
	'--package',
	'actionlint-plugin-ruff-component',
	'--target',
	'wasm32-unknown-unknown',
]);
await packageComponent();
