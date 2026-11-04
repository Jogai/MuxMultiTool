import { create } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";

import { canStart, mismatchesOfPart, toggleDrop, toOperation } from "./merge";
import { CheckMergeResponseSchema, MergeMismatchSchema } from "~/generated/muxmultitool/merge/v1/merge_pb";

describe("merge helpers", () => {
	it("toggles dropped streams in index order", () => {
		expect(toggleDrop([], 2)).toEqual([2]);
		expect(toggleDrop([2], 1)).toEqual([1, 2]);
		expect(toggleDrop([1, 2], 1)).toEqual([2]);
	});

	it("builds the operation with a trimmed output", () => {
		const operation = toOperation({ inputs: ["a", "b"], dropStreams: [1], chapters: true, output: " out.mkv " });
		expect(operation.inputs).toEqual(["a", "b"]);
		expect(operation.output).toBe("out.mkv");
		expect(operation.dropStreams).toEqual([1]);
		expect(operation.chapters).toBe(true);
	});

	it("starts only for a passing check of the current inputs", () => {
		const ok = create(CheckMergeResponseSchema, { canMerge: true, parts: [{ path: "a" }, { path: "b" }] });
		expect(canStart(ok, 2)).toBe(true);
		expect(canStart(ok, 3)).toBe(false);
		expect(canStart(undefined, 2)).toBe(false);
		expect(canStart(create(CheckMergeResponseSchema, { canMerge: false, parts: ok.parts }), 2)).toBe(false);
	});

	it("picks the mismatches of one part", () => {
		const mismatches = [create(MergeMismatchSchema, { part: 2 }), create(MergeMismatchSchema, { part: 1 })];
		expect(mismatchesOfPart(mismatches, 2)).toHaveLength(1);
	});
});
