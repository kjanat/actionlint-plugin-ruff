import { arg, cli, CLIError, command } from '@kjanat/dreamcli';
import { createDenoAdapter } from '@kjanat/dreamcli/runtime';
import { fileURLToPath } from 'node:url';
import { gunzipSync } from 'node:zlib';
import { readDiscoveryMetadata } from './metadata.ts';

const defaultPath = fileURLToPath(new URL('../artifacts/plugin.wasm', import.meta.url));

export const inspectCommand = command('inspect')
	.description('Read embedded plugin metadata and schema without a WASM runtime')
	.arg(
		'file',
		arg.path({ mustExist: true, type: 'file' })
			.default(defaultPath)
			.describe('Raw or gzip WASM asset (defaults to artifacts/plugin.wasm; build with deno task build)'),
	)
	.action(async ({ args, out }) => {
		try {
			let bytes = await Deno.readFile(args.file);
			if (bytes[0] === 0x1f && bytes[1] === 0x8b) bytes = new Uint8Array(gunzipSync(bytes));
			out.json(readDiscoveryMetadata(bytes));
		} catch (cause) {
			throw new CLIError(`Cannot inspect ${args.file}: ${cause instanceof Error ? cause.message : String(cause)}`, {
				code: 'INVALID_PLUGIN',
				cause,
			});
		}
	});

export const inspectCli = cli('inspect')
	.description('Read embedded plugin metadata and schema without a WASM runtime')
	.default(inspectCommand);

if (import.meta.main) {
	// DreamCLI 4.1.0's default adapter reads the entire environment and does not
	// catch Deno's NotCapable error. Restrict it to terminal presentation settings.
	const terminalVariables = ['FORCE_COLOR', 'NO_COLOR', 'CI', 'TERM', 'NO_HYPERLINKS', 'FORCE_HYPERLINKS'];
	const terminalEnvironment: Record<string, string> = {};
	for (const name of terminalVariables) {
		const value = Deno.env.get(name);
		if (value !== undefined) terminalEnvironment[name] = value;
	}
	const adapter = createDenoAdapter({
		...Deno,
		env: {
			get: (name) => terminalEnvironment[name],
			toObject: () => terminalEnvironment,
		},
	});
	await inspectCli.run({ adapter });
}
