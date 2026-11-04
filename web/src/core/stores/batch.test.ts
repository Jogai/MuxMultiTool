import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it } from "vitest";

import { useBatchStore } from "./batch";

describe("useBatchStore", () => {
	beforeEach(() => setActivePinia(createPinia()));

	it("makes a preset the operation and forgets the last batch", () => {
		const store = useBatchStore();
		store.form = { ...store.form, folder: "inbox", folderLabel: "Inbox", rules: [] };
		store.submission = { jobs: [{ id: "j1", name: "a.mkv" }], skipped: [] };
		store.applyPreset("7");
		expect(store.form).toMatchObject({ source: "preset", presetId: "7", folder: "inbox", folderLabel: "Inbox" });
		expect(store.submission).toBeUndefined();
	});

	it("starts over on reset", () => {
		const store = useBatchStore();
		store.applyPreset("7");
		store.reset();
		expect(store.form.source).toBe("rules");
		expect(store.form.presetId).toBe("");
	});
});
