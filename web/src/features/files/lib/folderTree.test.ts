import { create } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";

import { childrenOf, folderNode, type LoadedFolder } from "./folderTree";
import { toAppError } from "~/core/libs/errors";
import { EntryKind, EntrySchema } from "~/generated/muxmultitool/files/v1/files_pb";

const folder = (id: string) => create(EntrySchema, { id, name: id, kind: EntryKind.FOLDER, path: id });

describe("folder tree rows", () => {
	it("shows a loading note under a folder whose subfolders were not asked for yet", () => {
		const rows = childrenOf(folderNode(folder("a")), new Map());
		expect(rows).toHaveLength(1);
		expect(rows?.[0]).toMatchObject({ kind: "note", note: "loading", id: "note:loading:a" });
	});

	it("shows the subfolders once loaded, or an empty note", () => {
		const children = [folderNode(folder("a/b")), folderNode(folder("a/c"))];
		const loaded = new Map<string, LoadedFolder>([
			["a", { children }],
			["x", { children: [] }]
		]);
		expect(childrenOf(folderNode(folder("a")), loaded)).toBe(children);
		expect(childrenOf(folderNode(folder("x")), loaded)?.[0]).toMatchObject({ kind: "note", note: "empty" });
	});

	it("shows the error under a folder that could not be listed", () => {
		const loaded = new Map<string, LoadedFolder>([["a", { error: toAppError(new Error("boom")) }]]);
		expect(childrenOf(folderNode(folder("a")), loaded)?.[0]).toMatchObject({ kind: "note", note: "error", message: "boom" });
	});

	it("gives notes no children, so they are leaves", () => {
		const [note] = childrenOf(folderNode(folder("a")), new Map()) ?? [];
		expect(childrenOf(note!, new Map())).toBeUndefined();
	});
});
