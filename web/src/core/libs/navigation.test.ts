import { describe, expect, it } from "vitest";

import en from "../../../locales/en.json" with { type: "json" };
import { jobLink, NAV_ENTRIES, navNameForPath, SETTINGS_TABS, settingsTab } from "./navigation";

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

describe("settingsTab", () => {
	it("reads the tab from the hash, with or without #", () => {
		expect(settingsTab("#sbom")).toBe("sbom");
		expect(settingsTab("about")).toBe("about");
	});

	it("falls back to the first tab for no hash or an unknown one", () => {
		expect(settingsTab("")).toBe(SETTINGS_TABS[0]);
		expect(settingsTab("#")).toBe(SETTINGS_TABS[0]);
		expect(settingsTab("#nope")).toBe(SETTINGS_TABS[0]);
	});

	it("has a label for every tab", () => {
		for (const tab of SETTINGS_TABS) expect(en.pages.settings.tabs[tab]).toBeTruthy();
	});
});
