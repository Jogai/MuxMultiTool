import { describe, expect, it } from "vitest";

import { countByKind, filterSbom, parseSbom, type SbomComponent, sortSbom } from "./sbom";

const document = {
	bomFormat: "CycloneDX",
	specVersion: "1.6",
	metadata: {
		timestamp: "2026-11-07T12:00:00Z",
		component: { type: "application", name: "muxmultitool", version: "0.1.0" }
	},
	components: [
		{ type: "library", name: "tokio", version: "1.54.1", purl: "pkg:cargo/tokio@1.54.1", licenses: [{ expression: "MIT" }] },
		{ type: "library", name: "ring", version: "0.17.0", purl: "pkg:cargo/ring@0.17.0" },
		{
			type: "library",
			name: "@connectrpc/connect",
			version: "2.3.0",
			purl: "pkg:npm/%40connectrpc/connect@2.3.0",
			licenses: [{ expression: "Apache-2.0" }]
		},
		{
			type: "library",
			name: "odd",
			version: "1.0.0",
			purl: "pkg:npm/odd@1.0.0",
			licenses: [{ license: { name: "SEE LICENSE IN LICENSE.md" } }]
		},
		{ type: "application", name: "ffmpeg", version: "9.0.2", licenses: [{ expression: "GPL-3.0-or-later" }] },
		{ type: "library", version: "1.0.0" }
	]
};

describe("parseSbom", () => {
	it("flattens the components with their kind and license", () => {
		const sbom = parseSbom(document);
		expect(sbom.application).toEqual({ name: "muxmultitool", version: "0.1.0" });
		expect(sbom.timestamp).toBe("2026-11-07T12:00:00Z");
		expect(sbom.components).toEqual<SbomComponent[]>([
			{ name: "tokio", version: "1.54.1", license: "MIT", kind: "cargo" },
			{ name: "ring", version: "0.17.0", license: "", kind: "cargo" },
			{ name: "@connectrpc/connect", version: "2.3.0", license: "Apache-2.0", kind: "npm" },
			{ name: "odd", version: "1.0.0", license: "SEE LICENSE IN LICENSE.md", kind: "npm" },
			{ name: "ffmpeg", version: "9.0.2", license: "GPL-3.0-or-later", kind: "application" }
		]);
	});

	it("rejects anything that is not a CycloneDX document", () => {
		expect(() => parseSbom(null)).toThrow(TypeError);
		expect(() => parseSbom("<!doctype html>")).toThrow(TypeError);
		expect(() => parseSbom({ bomFormat: "SPDX" })).toThrow(TypeError);
	});

	it("reads a document without components", () => {
		expect(parseSbom({ bomFormat: "CycloneDX" })).toEqual({ application: undefined, timestamp: undefined, components: [] });
	});
});

describe("filterSbom", () => {
	const components = parseSbom(document).components;

	it("matches the name or the license, ignoring case", () => {
		expect(filterSbom(components, "TOKIO").map((c) => c.name)).toEqual(["tokio"]);
		expect(filterSbom(components, "apache").map((c) => c.name)).toEqual(["@connectrpc/connect"]);
		expect(filterSbom(components, "gpl").map((c) => c.name)).toEqual(["ffmpeg"]);
	});

	it("needs every word", () => {
		expect(filterSbom(components, "connect apache").map((c) => c.name)).toEqual(["@connectrpc/connect"]);
		expect(filterSbom(components, "connect mit")).toEqual([]);
	});

	it("returns everything for an empty query", () => {
		expect(filterSbom(components, "  ")).toHaveLength(components.length);
	});
});

describe("countByKind", () => {
	it("counts every kind, zero included", () => {
		expect(countByKind(parseSbom(document).components)).toEqual({ cargo: 2, npm: 2, application: 1 });
		expect(countByKind([])).toEqual({ cargo: 0, npm: 0, application: 0 });
	});
});

describe("sortSbom", () => {
	it("puts programs first, then crates, then npm packages, each by name", () => {
		const names = sortSbom(parseSbom(document).components).map((c) => `${c.kind}:${c.name}`);
		expect(names).toEqual(["application:ffmpeg", "cargo:ring", "cargo:tokio", "npm:@connectrpc/connect", "npm:odd"]);
	});

	it("orders versions numerically", () => {
		const versions = sortSbom([
			{ name: "a", version: "1.10.0", license: "", kind: "cargo" },
			{ name: "a", version: "1.9.0", license: "", kind: "cargo" }
		]).map((c) => c.version);
		expect(versions).toEqual(["1.9.0", "1.10.0"]);
	});
});
