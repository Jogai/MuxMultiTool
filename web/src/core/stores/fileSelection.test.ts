import { create } from "@bufbuild/protobuf";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it } from "vitest";

import { useFileSelectionStore } from "./fileSelection";
import { EntryKind, EntrySchema } from "~/generated/muxmultitool/files/v1/files_pb";

const file = (id: string) => create(EntrySchema, { id, name: `${id}.mkv`, kind: EntryKind.FILE });

describe("useFileSelectionStore", () => {
	beforeEach(() => setActivePinia(createPinia()));

	it("keeps files in the order they were added, once each", () => {
		const selection = useFileSelectionStore();
		expect(selection.isEmpty).toBe(true);

		selection.add([file("b"), file("a")]);
		selection.add([file("a"), file("c")]);
		expect(selection.ids).toEqual(["b", "a", "c"]);
		expect(selection.count).toBe(3);
		expect(selection.has("a")).toBe(true);
	});

	it("toggles, removes and clears", () => {
		const selection = useFileSelectionStore();
		selection.toggle(file("a"));
		selection.toggle(file("b"));
		selection.toggle(file("a"));
		expect(selection.ids).toEqual(["b"]);

		selection.remove(["b", "missing"]);
		expect(selection.isEmpty).toBe(true);

		selection.set([file("x"), file("y")]);
		expect(selection.files.map((entry) => entry.name)).toEqual(["x.mkv", "y.mkv"]);
		selection.clear();
		expect(selection.count).toBe(0);
	});
});
