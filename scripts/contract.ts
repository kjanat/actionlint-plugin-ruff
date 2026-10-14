export interface InterfaceDescriptor {
	version: number;
	export: string;
	exclusive_group?: string;
}

export interface Metadata {
	abi_version: number;
	plugin_version: string;
	engine: { name: string; version: string };
	capabilities: {
		noqa: boolean;
		check?: boolean;
		validate_config?: boolean;
		config_schema?: boolean;
		fixes?: boolean;
		format?: boolean;
		filesystem?: boolean;
		position_encoding?: string;
		max_input_bytes?: number;
	};
	config_schema: { additionalProperties: boolean; properties: Record<string, unknown> };
	// Optional only for artifacts made before versioned discovery was introduced.
	metadata_version?: number;
	transport?: string;
	id?: string;
	name?: string;
	description?: string;
	license?: string;
	homepage?: string;
	repository?: string;
	documentation_url?: string;
	update_url?: string;
	update_format?: string;
	languages?: string[];
	shells?: string[];
	file_extensions?: string[];
	conflicts?: string[];
	interfaces?: Record<string, InterfaceDescriptor[]>;
}

const legacyExports: Readonly<Record<string, string>> = {
	check: 'check',
	validate_config: 'validate_config',
	metadata: 'metadata',
};

/** Select only an explicitly understood contract; unknown optional interfaces are ignored. */
export function resolveInterface(metadata: Metadata, name: string, version: number): string | undefined {
	if (metadata.abi_version !== 1) throw new Error(`unsupported ABI version: ${metadata.abi_version}`);
	if ((metadata.metadata_version ?? 1) !== 1) {
		throw new Error(`unsupported metadata version: ${metadata.metadata_version}`);
	}
	if (metadata.transport !== undefined && metadata.transport !== 'wasm-core-json') {
		throw new Error(`unsupported transport: ${metadata.transport}`);
	}
	if (metadata.interfaces === undefined && metadata.metadata_version === undefined) {
		return version === 1 && Object.hasOwn(legacyExports, name) ? legacyExports[name] : undefined;
	}
	if (metadata.interfaces === undefined || !Object.hasOwn(metadata.interfaces, name)) return undefined;
	return metadata.interfaces?.[name]?.find((entry) => entry.version === version)?.export;
}

export function requireInterface(metadata: Metadata, name: string, version: number): string {
	const entry = resolveInterface(metadata, name, version);
	if (entry === undefined) throw new Error(`unsupported interface: ${name} v${version}`);
	return entry;
}
