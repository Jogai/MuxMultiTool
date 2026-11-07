/** Where the server serves the CycloneDX document (crates/server/src/sbom.rs). */
export const SBOM_URL = "/sbom.cdx.json";

/** What a component is: a Rust crate, an npm package, or a program such as ffmpeg. */
export type SbomKind = "cargo" | "npm" | "application";

export const SBOM_KINDS: readonly SbomKind[] = ["cargo", "npm", "application"];

/** One row of the table: a component of the SBOM, flattened. */
export interface SbomComponent {
	name: string;
	version: string;
	/** The SPDX expression or the license name; empty when the component declares none. */
	license: string;
	kind: SbomKind;
}

/** The parts of the document the page shows. */
export interface Sbom {
	/** The application the document describes, from `metadata.component`. */
	application?: { name: string; version: string };
	/** The build time (`metadata.timestamp`), RFC 3339. */
	timestamp?: string;
	components: SbomComponent[];
}

/** The subset of CycloneDX 1.6 JSON this reads. Anything else in the document is ignored. */
interface CycloneDx {
	bomFormat?: unknown;
	metadata?: { timestamp?: unknown; component?: CycloneDxComponent };
	components?: CycloneDxComponent[];
}

interface CycloneDxComponent {
	type?: unknown;
	name?: unknown;
	version?: unknown;
	purl?: unknown;
	licenses?: { expression?: unknown; license?: { id?: unknown; name?: unknown } }[];
}

/**
 * Reads the components out of a parsed CycloneDX document. Throws when the value is not one, so a
 * proxy's HTML error page or an empty body shows as an error instead of an empty table.
 */
export function parseSbom(value: unknown): Sbom {
	const document = value as CycloneDx | null;
	if (typeof document !== "object" || document === null || document.bomFormat !== "CycloneDX") {
		throw new TypeError("not a CycloneDX document");
	}
	const components = (document.components ?? []).flatMap((component) => {
		const name = text(component.name);
		if (!name) return [];
		return [
			{
				name,
				version: text(component.version),
				license: licenseOf(component),
				kind: kindOf(component)
			}
		];
	});
	const root = document.metadata?.component;
	return {
		application: root && text(root.name) ? { name: text(root.name), version: text(root.version) } : undefined,
		timestamp: text(document.metadata?.timestamp) || undefined,
		components
	};
}

function text(value: unknown): string {
	return typeof value === "string" ? value : "";
}

/** The first license choice, as the expression or the named license. */
function licenseOf(component: CycloneDxComponent): string {
	for (const choice of component.licenses ?? []) {
		const value = text(choice.expression) || text(choice.license?.id) || text(choice.license?.name);
		if (value) return value;
	}
	return "";
}

/** The kind from the purl type, falling back to the component type for programs without one. */
function kindOf(component: CycloneDxComponent): SbomKind {
	const purl = text(component.purl);
	if (purl.startsWith("pkg:cargo/")) return "cargo";
	if (purl.startsWith("pkg:npm/")) return "npm";
	return "application";
}

/** Case-insensitive match of the name or the license against every word of the query. */
export function filterSbom(components: readonly SbomComponent[], query: string): SbomComponent[] {
	const words = query.toLowerCase().split(/\s+/).filter(Boolean);
	if (words.length === 0) return [...components];
	return components.filter((component) => {
		const haystack = `${component.name} ${component.license}`.toLowerCase();
		return words.every((word) => haystack.includes(word));
	});
}

/** How many components of each kind, every kind present even when zero. */
export function countByKind(components: readonly SbomComponent[]): Record<SbomKind, number> {
	const counts: Record<SbomKind, number> = { cargo: 0, npm: 0, application: 0 };
	for (const component of components) counts[component.kind] += 1;
	return counts;
}

/** The table's order: programs first (ffmpeg), then the crates, then the npm packages, each by name. */
const KIND_ORDER: readonly SbomKind[] = ["application", "cargo", "npm"];

export function sortSbom(components: readonly SbomComponent[]): SbomComponent[] {
	return [...components].sort(
		(a, b) =>
			KIND_ORDER.indexOf(a.kind) - KIND_ORDER.indexOf(b.kind) ||
			a.name.localeCompare(b.name) ||
			a.version.localeCompare(b.version, undefined, { numeric: true })
	);
}
