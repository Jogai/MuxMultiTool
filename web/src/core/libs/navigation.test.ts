import { describe, expect, it } from "vitest";

import { NAV_ENTRIES, navNameForPath } from "./navigation";

describe("NAV_ENTRIES", () => {
	it("lists the rail entries from the sketches in order", () => {
		expect(NAV_ENTRIES.map((entry) => entry.name)).toEqual([
			"files",
			"streams",
			"combine",
			"cut",
			"merge",
			"images",
			"batch",
			"presets",
			"jobs",
			"settings"
		]);
	});
});

describe("navNameForPath", () => {
	it("matches the entry and its sub-pages", () => {
		expect(navNameForPath("/files")).toBe("files");
		expect(navNameForPath("/settings/about")).toBe("settings");
		expect(navNameForPath("/")).toBeUndefined();
		expect(navNameForPath("/filesystem")).toBeUndefined();
	});
});
