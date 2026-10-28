import { create } from "@bufbuild/protobuf";
import { describe, expect, it, vi } from "vitest";

import { entriesForPicked, pickedFromEntry } from "./pickedFiles";
import { EntryKind, EntrySchema, GetEntriesResponseSchema, OpenPathsResponseSchema } from "~/generated/muxmultitool/files/v1/files_pb";
import { fakeFilesClient } from "~/test/fakeFilesClient";

describe("entriesForPicked", () => {
	it("looks container ids up again and drops what is gone", async () => {
		const client = fakeFilesClient({
			getEntries: vi.fn().mockResolvedValue(
				create(GetEntriesResponseSchema, {
					entries: [
						{ id: "a", name: "a.mkv", kind: EntryKind.FILE },
						{ id: "gone", kind: EntryKind.UNSPECIFIED }
					]
				})
			)
		});
		const entries = await entriesForPicked(client, "container", [
			{ path: "a", name: "a.mkv" },
			{ path: "gone", name: "gone.mkv" }
		]);
		expect(entries.map((entry) => entry.id)).toEqual(["a"]);
		expect(client.getEntries).toHaveBeenCalledWith({ ids: ["a", "gone"] });
		expect(client.openPaths).not.toHaveBeenCalled();
	});

	it("opens native paths on desktop", async () => {
		const client = fakeFilesClient({
			openPaths: vi.fn().mockResolvedValue(create(OpenPathsResponseSchema, { entries: [{ id: "x", name: "x.mp4", kind: EntryKind.FILE }] }))
		});
		const entries = await entriesForPicked(client, "desktop", [{ path: "/home/me/x.mp4", name: "x.mp4" }]);
		expect(entries.map((entry) => entry.id)).toEqual(["x"]);
		expect(client.openPaths).toHaveBeenCalledWith({ paths: ["/home/me/x.mp4"] });
	});

	it("asks nothing for an empty pick", async () => {
		const client = fakeFilesClient();
		await expect(entriesForPicked(client, "container", [])).resolves.toEqual([]);
		expect(client.getEntries).not.toHaveBeenCalled();
	});
});

describe("pickedFromEntry", () => {
	it("hands the id back as the path", () => {
		expect(pickedFromEntry(create(EntrySchema, { id: "abc", name: "a.mkv", path: "Movies/a.mkv" }))).toEqual({
			path: "abc",
			name: "a.mkv"
		});
	});
});
