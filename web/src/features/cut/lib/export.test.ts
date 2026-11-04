import { describe, expect, it } from "vitest";

import { CutExport, CutMode, SegmentMode } from "~/generated/muxmultitool/cut/v1/cut_pb";

import { canExport, defaultOutput, isDefaultOutput, splitName, toCutOperation } from "./export";
import type { EditorState } from "./segments";

const state: EditorState = {
	segments: [
		{ id: 1, start: 10, end: 20 },
		{ id: 2, start: 30, end: 45 }
	],
	segmentMode: SegmentMode.REMOVE,
	cutMode: CutMode.NORMAL,
	export: CutExport.MERGED
};

describe("cut export", () => {
	it("builds the operation from the editor state", () => {
		const operation = toCutOperation("/media/a.mkv", state, " /media/out.mkv ");
		expect(operation.input).toBe("/media/a.mkv");
		expect(operation.output).toBe("/media/out.mkv");
		expect(operation.segments).toMatchObject([
			{ startSeconds: 10, endSeconds: 20 },
			{ startSeconds: 30, endSeconds: 45 }
		]);
		expect(operation.segmentMode).toBe(SegmentMode.REMOVE);
		expect(operation.cutMode).toBe(CutMode.NORMAL);
		expect(operation.export).toBe(CutExport.MERGED);
	});

	it("needs segments and an output", () => {
		expect(canExport(state, "/media/out.mkv")).toBe(true);
		expect(canExport(state, "  ")).toBe(false);
		expect(canExport({ ...state, segments: [] }, "/media/out.mkv")).toBe(false);
	});

	it("splits a path into folder, stem and extension", () => {
		expect(splitName("/media/movies/Movie A (2024).mkv")).toEqual({ folder: "/media/movies/", stem: "Movie A (2024)", extension: ".mkv" });
		expect(splitName("clip")).toEqual({ folder: "", stem: "clip", extension: "" });
		expect(splitName("C:\\Videos\\.hidden")).toEqual({ folder: "C:\\Videos\\", stem: ".hidden", extension: "" });
	});

	it("suggests an output next to the source per export mode", () => {
		expect(defaultOutput("/media/a.mkv", CutExport.MERGED)).toBe("/media/a-cut.mkv");
		expect(defaultOutput("/media/a.mkv", CutExport.SEPARATE_FILES)).toBe("/media/a-cut-{n}.mkv");
		expect(defaultOutput("", CutExport.MERGED)).toBe("");
		expect(isDefaultOutput("/media/a-cut.mkv", "/media/a.mkv")).toBe(true);
		expect(isDefaultOutput("/media/a-cut-{n}.mkv", "/media/a.mkv")).toBe(true);
		expect(isDefaultOutput("/media/mine.mkv", "/media/a.mkv")).toBe(false);
	});
});
