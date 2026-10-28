import { describe, expect, it, vi } from "vitest";

import { openExternal } from "./externalLink";

describe("openExternal", () => {
	it("leaves the link to the browser outside the desktop app", () => {
		expect(openExternal("https://example.org", {})).toBe(false);
	});

	it("hands the url to the system browser on desktop", () => {
		const invoke = vi.fn().mockResolvedValue(undefined);
		expect(openExternal("https://example.org", { __TAURI_INTERNALS__: { invoke } })).toBe(true);
		expect(invoke).toHaveBeenCalledWith("plugin:opener|open_url", { url: "https://example.org" });
	});
});
