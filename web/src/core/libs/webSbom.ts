/**
 * The npm part of the software bill of materials, which `vite build` writes as `sbom.web.json` and the
 * server merges into `/sbom.cdx.json` (crates/server/src/sbom.rs, development.md).
 *
 * It lists the direct dependencies of package.json only: `dependencies` are bundled into the page
 * (runtime), `devDependencies` only build, check or test it (build time, `dev: true`). The packages
 * they pull in are left out. The version is the one the lock file installs, not the range.
 */

/** The parts of package.json this reads. */
export interface Manifest {
	dependencies?: Record<string, string>;
	devDependencies?: Record<string, string>;
}

/** The parts of package-lock.json (lockfileVersion 3) this reads. */
export interface LockFile {
	packages?: Record<string, { name?: string; version?: string; license?: string }>;
}

/** One entry of `sbom.web.json`. */
export interface WebSbomPackage {
	name: string;
	version: string;
	license?: string;
	/** A devDependency: used while building, not in the page. */
	dev?: true;
}

/**
 * The direct dependencies with their installed versions, sorted by name then version. A dependency
 * the lock file does not install is skipped. An alias (`"typescript7": "npm:typescript@^7"`) is listed
 * under the real package name. A package in both sections counts as runtime.
 */
export function webSbomPackages(manifest: Manifest, lock: LockFile): WebSbomPackage[] {
	const sections: [Record<string, string> | undefined, boolean][] = [
		[manifest.dependencies, false],
		[manifest.devDependencies, true]
	];
	const byKey = new Map<string, WebSbomPackage>();
	for (const [section, dev] of sections) {
		for (const key of Object.keys(section ?? {})) {
			const entry = lock.packages?.[`node_modules/${key}`];
			if (!entry?.version) continue;
			const name = entry.name ?? key;
			const id = `${name}@${entry.version}`;
			if (byKey.has(id)) continue;
			byKey.set(id, {
				name,
				version: entry.version,
				...(entry.license ? { license: entry.license } : {}),
				...(dev ? { dev: true as const } : {})
			});
		}
	}
	return [...byKey.values()].sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));
}
