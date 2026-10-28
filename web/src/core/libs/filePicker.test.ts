import { describe, expect, it, vi } from "vitest";

import { appMode, baseName, createContainerFilePicker, createDesktopFilePicker, createFilePicker, isDesktop, joinPath } from "./filePicker";

const tauriHost = { __TAURI_INTERNALS__: {} };

/** A Tauri host whose IPC answers every command with `response`. */
function desktopHost(response: unknown) {
	const invoke = vi.fn().mockResolvedValue(response);
	return { host: { __TAURI_INTERNALS__: { invoke } }, invoke };
}

describe("mode detection", () => {
	it("sees the desktop app by Tauri's injected global", () => {
		expect(isDesktop(tauriHost)).toBe(true);
		expect(isDesktop({})).toBe(false);
		expect(appMode(tauriHost)).toBe("desktop");
		expect(appMode({})).toBe("container");
	});

	it("is in container mode under the test runner", () => {
		expect(appMode()).toBe("container");
	});
});

describe("baseName", () => {
	it("takes the last segment with either separator", () => {
		expect(baseName("/media/Movies/Movie A (2024).mkv")).toBe("Movie A (2024).mkv");
		expect(baseName("C:\\Videos\\clip.mp4")).toBe("clip.mp4");
		expect(baseName("/media/out/")).toBe("out");
		expect(baseName("plain.mkv")).toBe("plain.mkv");
	});
});

describe("joinPath", () => {
	it("keeps the folder's separator and avoids a double one", () => {
		expect(joinPath("/media/out", "a.mkv")).toBe("/media/out/a.mkv");
		expect(joinPath("/media/out/", "a.mkv")).toBe("/media/out/a.mkv");
		expect(joinPath("C:\\Videos", "a.mkv")).toBe("C:\\Videos\\a.mkv");
		expect(joinPath("C:\\Videos\\", "a.mkv")).toBe("C:\\Videos\\a.mkv");
	});
});

describe("container picker", () => {
	it("delegates to the in-app browser with the defaults filled in", async () => {
		const open = vi.fn().mockResolvedValue([{ path: "/media/a.mkv", name: "a.mkv" }]);
		const picker = createContainerFilePicker(open);

		await expect(picker.pickFiles({ startIn: "/media" })).resolves.toEqual([{ path: "/media/a.mkv", name: "a.mkv" }]);
		expect(picker.mode).toBe("container");
		expect(open).toHaveBeenCalledWith({ multiple: true, directory: false, startIn: "/media" });
	});

	it("lets the caller override the defaults", async () => {
		const open = vi.fn().mockResolvedValue([]);
		await createContainerFilePicker(open).pickFiles({ multiple: false, directory: true });

		expect(open).toHaveBeenCalledWith({ multiple: false, directory: true });
	});

	it("saves into a folder from the browser with the suggested name", async () => {
		const open = vi.fn().mockResolvedValue([{ path: "/media/out", name: "out" }]);
		const picker = createContainerFilePicker(open);

		await expect(picker.pickSavePath({ defaultName: "cut.mkv", startIn: "/media" })).resolves.toEqual({
			path: "/media/out/cut.mkv",
			name: "cut.mkv"
		});
		expect(open).toHaveBeenCalledWith({ multiple: false, directory: true, startIn: "/media" });

		open.mockResolvedValue([]);
		await expect(picker.pickSavePath({ defaultName: "cut.mkv" })).resolves.toBeUndefined();
	});
});

describe("desktop picker", () => {
	it("opens the native dialog over the dialog plugin's IPC command", async () => {
		const { host, invoke } = desktopHost(["C:\\Videos\\a.mkv", "C:\\Videos\\b.mp4"]);
		const picker = createDesktopFilePicker(host);

		expect(picker.mode).toBe("desktop");
		await expect(picker.pickFiles({ startIn: "C:\\Videos", extensions: ["mkv", "mp4"] })).resolves.toEqual([
			{ path: "C:\\Videos\\a.mkv", name: "a.mkv" },
			{ path: "C:\\Videos\\b.mp4", name: "b.mp4" }
		]);
		expect(invoke).toHaveBeenCalledWith("plugin:dialog|open", {
			options: {
				multiple: true,
				directory: false,
				defaultPath: "C:\\Videos",
				filters: [{ name: "*.mkv, *.mp4", extensions: ["mkv", "mp4"] }]
			}
		});
	});

	it("passes no filter when every file is accepted", async () => {
		const { host, invoke } = desktopHost(null);
		await createDesktopFilePicker(host).pickFiles();

		expect(invoke).toHaveBeenCalledWith("plugin:dialog|open", {
			options: { multiple: true, directory: false, defaultPath: undefined, filters: [] }
		});
	});

	it("maps a single path and a folder to one entry", async () => {
		const single = desktopHost("/Users/me/Movies/a.mkv");
		await expect(createDesktopFilePicker(single.host).pickFiles({ multiple: false })).resolves.toEqual([
			{ path: "/Users/me/Movies/a.mkv", name: "a.mkv" }
		]);

		const folder = desktopHost("/Users/me/Movies");
		await expect(createDesktopFilePicker(folder.host).pickFiles({ multiple: false, directory: true })).resolves.toEqual([
			{ path: "/Users/me/Movies", name: "Movies" }
		]);
		expect(folder.invoke).toHaveBeenCalledWith(
			"plugin:dialog|open",
			expect.objectContaining({ options: expect.objectContaining({ directory: true }) })
		);
	});

	it("resolves with nothing when the user cancels", async () => {
		const { host } = desktopHost(null);
		const picker = createDesktopFilePicker(host);

		await expect(picker.pickFiles()).resolves.toEqual([]);
		await expect(picker.pickSavePath({ defaultName: "cut.mkv" })).resolves.toBeUndefined();
	});

	it("asks the native save dialog with the suggested name in the start folder", async () => {
		const { host, invoke } = desktopHost("/Users/me/Movies/cut.mkv");
		const picker = createDesktopFilePicker(host);

		await expect(picker.pickSavePath({ defaultName: "cut.mkv", startIn: "/Users/me/Movies", extensions: ["mkv"] })).resolves.toEqual({
			path: "/Users/me/Movies/cut.mkv",
			name: "cut.mkv"
		});
		expect(invoke).toHaveBeenCalledWith("plugin:dialog|save", {
			options: { defaultPath: "/Users/me/Movies/cut.mkv", filters: [{ name: "*.mkv", extensions: ["mkv"] }] }
		});

		await picker.pickSavePath({ defaultName: "cut.mkv" });
		expect(invoke).toHaveBeenLastCalledWith("plugin:dialog|save", { options: { defaultPath: "cut.mkv", filters: [] } });
	});

	it("rejects when the dialog command is denied", async () => {
		const invoke = vi.fn().mockRejectedValue(new Error("dialog.open not allowed"));
		await expect(createDesktopFilePicker({ __TAURI_INTERNALS__: { invoke } }).pickFiles()).rejects.toThrow(/not allowed/);
	});
});

describe("createFilePicker", () => {
	it("picks the implementation by mode", () => {
		const open = vi.fn();
		expect(createFilePicker(open, {}).mode).toBe("container");
		expect(createFilePicker(open, tauriHost).mode).toBe("desktop");
	});
});
