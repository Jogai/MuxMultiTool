import { describe, expect, it, vi } from "vitest";

import { appMode, baseName, createContainerFilePicker, createDesktopFilePicker, createFilePicker, isDesktop } from "./filePicker";

const tauriHost = { __TAURI_INTERNALS__: {} };

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
});

describe("desktop picker", () => {
	it("rejects until the Tauri task wires the native dialog", async () => {
		const picker = createDesktopFilePicker();
		expect(picker.mode).toBe("desktop");
		await expect(picker.pickFiles()).rejects.toThrow(/TASK-44/);
	});
});

describe("createFilePicker", () => {
	it("picks the implementation by mode", () => {
		const open = vi.fn();
		expect(createFilePicker(open, {}).mode).toBe("container");
		expect(createFilePicker(open, tauriHost).mode).toBe("desktop");
	});
});
