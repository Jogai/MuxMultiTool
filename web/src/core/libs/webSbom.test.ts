import { describe, expect, it } from "vitest";
import { webSbomPackages } from "./webSbom";

describe("webSbomPackages", () => {
	const lock = {
		packages: {
			"": { name: "web", version: "0.2.0" },
			"node_modules/vue": { version: "3.5.45", license: "MIT" },
			"node_modules/@vue/shared": { version: "3.5.45", license: "MIT" },
			"node_modules/vite": { version: "8.3.8", license: "MIT" },
			"node_modules/typescript": { version: "6.0.3", license: "Apache-2.0" },
			"node_modules/typescript7": { name: "typescript", version: "7.0.2", license: "Apache-2.0" },
			"node_modules/nolicense": { version: "1.0.0" }
		}
	};

	it("lists dependencies as runtime and devDependencies as build time, nothing transitive", () => {
		const packages = webSbomPackages({ dependencies: { vue: "^3.5.0" }, devDependencies: { vite: "^8.0.0" } }, lock);
		expect(packages).toEqual([
			{ name: "vite", version: "8.3.8", license: "MIT", dev: true },
			{ name: "vue", version: "3.5.45", license: "MIT" }
		]);
	});

	it("lists an alias under the real name, next to the package it aliases", () => {
		const packages = webSbomPackages({ devDependencies: { typescript: "^6.0.3", typescript7: "npm:typescript@^7.0.2" } }, lock);
		expect(packages.map((p) => `${p.name}@${p.version}`)).toEqual(["typescript@6.0.3", "typescript@7.0.2"]);
	});

	it("counts a package in both sections once, as runtime", () => {
		const packages = webSbomPackages({ dependencies: { vue: "^3" }, devDependencies: { vue: "^3" } }, lock);
		expect(packages).toEqual([{ name: "vue", version: "3.5.45", license: "MIT" }]);
	});

	it("skips what the lock file does not install and leaves out a missing license", () => {
		const packages = webSbomPackages({ dependencies: { missing: "^1", nolicense: "^1" } }, lock);
		expect(packages).toEqual([{ name: "nolicense", version: "1.0.0" }]);
	});

	it("handles a manifest without sections", () => {
		expect(webSbomPackages({}, {})).toEqual([]);
	});
});
