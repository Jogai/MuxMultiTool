import { describe, expect, it } from "vitest";

import { jobLink, NAV_ENTRIES, navNameForPath } from "./navigation";

describe("NAV_ENTRIES", () => {
	it("lists the rail entries in order", () => {
		expect(NAV_ENTRIES.map((entry) => entry.name)).toEqual([
			"files",
			"streams",
			"combine",
			"cut",
			"merge",
			"crop",
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

describe("jobLink", () => {
	it("links to the job on the Jobs screen", () => {
		expect(jobLink("j1")).toEqual({ path: "/jobs", query: { job: "j1" } });
	});
});
