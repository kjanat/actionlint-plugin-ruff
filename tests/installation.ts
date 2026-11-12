import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';

interface Context {
	directory: string;
	run: (args: string[], expected?: number) => Promise<string>;
	runResult: (args: string[], expected?: number) => Promise<{ stdout: string; stderr: string }>;
}

const workflow = `on: push
permissions: {}
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - shell: python
        run: print(missing)
`;

export async function prepare({ directory }: Pick<Context, 'directory'>): Promise<void> {
	await writeFile(join(directory, 'workflow.yml'), workflow);
}

function object(value: unknown): Record<string, unknown> {
	assert.ok(typeof value === 'object' && value !== null && !Array.isArray(value));
	return Object.fromEntries(Object.entries(value));
}

export async function verify({ run }: Context): Promise<{ checks: string[]; probe: string[] }> {
	const probe = ['check', '--json', '--no-tool', 'shellcheck', 'workflow.yml'];
	const result = object(JSON.parse(await run(probe, 1)));
	assert.equal(result.completed, true);
	assert.equal(result.exit_code, 1);
	assert.ok(Array.isArray(result.diagnostics));
	assert.equal(result.diagnostics.length, 1);
	const diagnostic = object(result.diagnostics[0]);
	assert.equal(diagnostic.rule, 'ruff');
	assert.equal(diagnostic.code, 'F821');
	assert.equal(diagnostic.message, 'Undefined name `missing`');
	assert.deepEqual(diagnostic.start, { line: 8, column: 20 });
	assert.deepEqual(diagnostic.end, { line: 8, column: 27 });
	return { checks: ['F821 through the Python script engine', 'F821 workflow position'], probe };
}
