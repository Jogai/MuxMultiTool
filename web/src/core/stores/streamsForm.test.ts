import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it } from "vitest";

import { useStreamsFormStore } from "./streamsForm";
import { StreamType } from "~/generated/muxmultitool/files/v1/files_pb";

describe("useStreamsFormStore", () => {
	beforeEach(() => setActivePinia(createPinia()));

	it("applies a preset's part, keeps the folder and drops the ticks", () => {
		const store = useStreamsFormStore();
		store.form = { ...store.form, folder: "/media/out", overrides: new Map([["a", new Set([1])]]) };
		store.applyPreset({
			action: "copy",
			videoOnly: false,
			rules: [{ action: "drop", streamType: StreamType.AUDIO, match: "language", value: "fre" }],
			untagged: "drop",
			container: "mp4",
			template: "{name}.{lang}.{ext}"
		});
		expect(store.form).toMatchObject({ folder: "/media/out", container: "mp4", template: "{name}.{lang}.{ext}", untagged: "drop" });
		expect(store.form.rules).toHaveLength(1);
		expect(store.form.overrides.size).toBe(0);
	});

	it("starts over on reset", () => {
		const store = useStreamsFormStore();
		store.applyPreset({ action: "extract", videoOnly: true, rules: [], untagged: "keep", container: "mkv", template: "{name}.{ext}" });
		store.reset();
		expect(store.form.action).toBe("copy");
		expect(store.form.videoOnly).toBe(false);
	});
});
