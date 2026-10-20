import { runCommand } from '@kjanat/dreamcli/testkit';
import { deepStrictEqual as equal, match, ok } from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { inspectCli, inspectCommand } from '../scripts/inspect.ts';

const asset = (name: string) => fileURLToPath(new URL(`../artifacts/${name}`, import.meta.url));
const options = {
	stat: async (path: string): Promise<'file' | 'directory' | null> => {
		try {
			const info = await Deno.stat(path);
			return info.isFile ? 'file' : info.isDirectory ? 'directory' : null;
		} catch (error) {
			if (error instanceof Deno.errors.NotFound) return null;
			throw error;
		}
	},
};

Deno.test('inspect defaults to the packaged artifact and emits metadata JSON', async () => {
	const result = await runCommand(inspectCommand, [], options);
	equal(result.exitCode, 0);
	equal(result.stderr, []);
	equal(JSON.parse(result.stdout.join('\n')), JSON.parse(await Deno.readTextFile(asset('metadata.json'))));
});

Deno.test('inspect accepts explicit raw and gzip assets, including JSON mode', async () => {
	const expected = JSON.parse(await Deno.readTextFile(asset('metadata.json')));
	for (const name of ['plugin.wasm', 'plugin.wasm.gz']) {
		for (const flags of [[], ['--json']]) {
			const result = await runCommand(inspectCommand, [...flags, asset(name)], options);
			equal(result.exitCode, 0);
			equal(result.stderr, []);
			equal(JSON.parse(result.stdout.join('\n')), expected);
		}
	}
});

Deno.test('inspect reads typed component metadata from raw and gzip assets', async () => {
	const expected = JSON.parse(await Deno.readTextFile(asset('component.metadata.json')));
	for (const name of ['plugin.component.wasm', 'plugin.component.wasm.gz']) {
		const result = await runCommand(inspectCommand, [asset(name)], options);
		equal(result.exitCode, 0);
		equal(result.stderr, []);
		equal(JSON.parse(result.stdout.join('\n')), expected);
	}
});

Deno.test('inspect help explains the optional file without reading it', async () => {
	const result = await inspectCli.execute(['--help'], {
		stat: () => {
			throw new Error('help must not probe files');
		},
	});
	equal(result.exitCode, 0);
	match(result.stdout.join('\n'), /\[file\]/);
	match(result.stdout.join('\n'), /artifacts\/plugin\.wasm/);
});

Deno.test('inspect rejects missing paths and directories before reading', async () => {
	for (const path of [asset('missing-inspect-fixture.wasm'), fileURLToPath(new URL('../scripts/', import.meta.url))]) {
		const result = await runCommand(inspectCommand, [path], options);
		ok(result.exitCode !== 0);
		ok(result.error);
		equal(result.stdout, []);
	}
});

Deno.test('inspect reports invalid plugin bytes as a structured error', async () => {
	const path = fileURLToPath(new URL('../scripts/inspect.ts', import.meta.url));
	const result = await runCommand(inspectCommand, [path], options);
	equal(result.exitCode, 1);
	equal(result.error?.code, 'INVALID_PLUGIN');
	match(result.error?.message ?? '', /Cannot inspect .*WASM v1 header/);
	equal(result.stdout, []);
});

Deno.test('inspect rejects extra arguments and unknown flags', async () => {
	for (const argv of [[asset('plugin.wasm'), 'extra.wasm'], ['--unknown']]) {
		const result = await inspectCli.execute(argv, options);
		ok(result.exitCode !== 0);
		ok(result.error);
		equal(result.stdout, []);
	}
});
