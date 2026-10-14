import { deepStrictEqual as equal } from 'node:assert/strict';
import { readMetadata } from '../scripts/metadata.ts';
import { Ruff } from '../scripts/wasm.ts';
import type { CheckResponse, CheckSuccess } from '../scripts/wasm.ts';

const wasm = await Deno.readFile(Deno.args[0] ?? new URL('../artifacts/plugin.wasm', import.meta.url));
const module = await WebAssembly.compile(wasm);

function checked(result: CheckResponse): CheckSuccess {
	if (!result.ok) throw new Error(`check failed: ${result.errors.join(', ')}`);
	return result;
}

Deno.test('module instantiates without WASI or JavaScript imports', () => {
	const engine = new Ruff(module);
	const metadata = engine.metadata();
	equal(readMetadata(wasm), metadata);
	equal(metadata.abi_version, 1);
	equal(metadata.engine.name, 'ruff');
	equal(metadata.capabilities.noqa, false);
	equal(metadata.metadata_version, 1);
	equal(metadata.id, 'actionlint-plugin-ruff');
	equal(metadata.languages, ['python']);
	equal(metadata.shells, ['python']);
	equal(metadata.capabilities.format, false);
	equal(metadata.capabilities.fixes, false);
	equal(metadata.config_schema.additionalProperties, false);
	equal(metadata.config_schema.properties.extend, undefined);
});

Deno.test('real WASM diagnostics preserve Unicode coordinates, CRLF and ignore noqa', () => {
	const result = checked(
		new Ruff(module).check("label = 'é🦀'; print(missing)  # noqa\r\n", { lint: { select: ['F821'] } }),
	);
	equal(result.ok, true);
	equal(result.diagnostics.map((d) => d.code), ['F821']);
	equal(result.diagnostics[0].location, { row: 1, column: 21 });
	equal(result.diagnostics[0].end_location, { row: 1, column: 28 });
});

Deno.test('config validation, syntax errors and empty selection run through Ruff', () => {
	const engine = new Ruff(module);
	equal(engine.validate({ lint: { select: ['F821'] }, 'target-version': 'py314' }).ok, true);
	equal(engine.validate({ lint: { select: ['FAKE999'] } }).ok, false);
	equal(engine.validate({ 'target-version': 'py999' }).ok, false);
	equal(engine.validate({ extend: 'ruff.toml' }).ok, false);
	equal(checked(engine.check('import os\n', { lint: { select: [] } })).diagnostics, []);
	equal(checked(engine.check('if :\n', { select: [] })).diagnostics[0].code, 'invalid-syntax');
	equal(engine.validate({}), { ok: true, errors: [] });
	const failed = engine.check('pass\n', { select: ['FAKE999'] });
	equal(failed.ok, false);
	if (failed.ok) throw new Error('invalid config must fail');
	equal(typeof failed.errors[0], 'string');
	equal('diagnostics' in failed, false);
});

Deno.test('bad JSON and bogus allocation handles return errors without trapping', () => {
	const engine = new Ruff(module);
	equal(engine.raw('check', 'garbage').ok, false);
	equal(engine.invalidInput(0, 100).ok, false);
	equal(engine.invalidInput(0xffffffff, 0xffffffff).ok, false);
	equal(engine.check('pass\n').ok, true);
});

Deno.test('all stable Ruff rules can run without filesystem or clock imports', () => {
	const result = checked(new Ruff(module).check('import imaginary_module\nimport os\nprint(missing)\n', {
		lint: { select: ['ALL'] },
	}));
	equal(result.ok, true);
	if (!result.diagnostics.some((diagnostic) => diagnostic.code === 'F821')) {
		throw new Error('ALL selection must include undefined-name checks');
	}
});

Deno.test('Ruff owns default selection while explicit selectors can replace it', () => {
	const engine = new Ruff(module);
	const source = 'import sys\nimport os\n\nprint(os.name, sys.version)\n';
	if (!checked(engine.check(source)).diagnostics.some((diagnostic) => diagnostic.code === 'I001')) {
		throw new Error('The linked Ruff default selection must include import sorting');
	}
	equal(checked(engine.check(source, { lint: { select: ['F'] } })).diagnostics, []);
	const metadata = engine.metadata();
	equal((metadata.config_schema.properties.select as Record<string, unknown>).default, undefined);
});

Deno.test('one instance supports repeated checks with bounded retained memory', () => {
	const engine = new Ruff(module);
	engine.metadata();
	engine.check('import os\nprint(missing)\n');
	const baseline = engine.memoryBytes;
	for (let i = 0; i < 200; i++) {
		equal(checked(engine.check('import os\n', { select: [] })).diagnostics, []);
		equal(checked(engine.check('print(missing) # noqa\n')).diagnostics[0].code, 'F821');
	}
	if (engine.memoryBytes > baseline + 1024 * 1024) {
		throw new Error(`memory grew from ${baseline} to ${engine.memoryBytes}`);
	}
});
