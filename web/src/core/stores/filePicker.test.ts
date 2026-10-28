import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it } from "vitest";

import { useFilePickerStore } from "./filePicker";

describe("useFilePickerStore", () => {
	beforeEach(() => setActivePinia(createPinia()));

	it("opens on a request and resolves it with the chosen files", async () => {
		const store = useFilePickerStore();
		expect(store.isOpen).toBe(false);

		const result = store.request({ multiple: true });
		expect(store.isOpen).toBe(true);
		expect(store.pending?.options).toEqual({ multiple: true });

		store.finish([{ path: "/media/a.mkv", name: "a.mkv" }]);
		await expect(result).resolves.toEqual([{ path: "/media/a.mkv", name: "a.mkv" }]);
		expect(store.isOpen).toBe(false);
	});

	it("resolves with nothing when cancelled", async () => {
		const store = useFilePickerStore();
		const result = store.request({});
		store.cancel();
		await expect(result).resolves.toEqual([]);
	});

	it("cancels an earlier request when a new one arrives", async () => {
		const store = useFilePickerStore();
		const first = store.request({});
		const second = store.request({ directory: true });

		await expect(first).resolves.toEqual([]);
		store.finish([{ path: "/media/out", name: "out" }]);
		await expect(second).resolves.toEqual([{ path: "/media/out", name: "out" }]);
	});
});
