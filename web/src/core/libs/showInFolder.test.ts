import { describe, expect, it, vi } from "vitest";

import { canShowInFolder, showInFolder } from "./showInFolder";

describe("showInFolder", () => {
	it("is only offered in the desktop window", () => {
		expect(canShowInFolder({})).toBe(false);
		expect(canShowInFolder({ __TAURI_INTERNALS__: { invoke: vi.fn() } })).toBe(true);
	});

	it("reveals the path through the opener plugin", async () => {
		const invoke = vi.fn().mockResolvedValue(null);
		await showInFolder("/home/me/clip.gif", { __TAURI_INTERNALS__: { invoke } });
		expect(invoke).toHaveBeenCalledWith("plugin:opener|reveal_item_in_dir", { path: "/home/me/clip.gif" });
	});

	it("refuses outside the desktop app", async () => {
		await expect(showInFolder("/x", {})).rejects.toThrow(/desktop/);
	});
});
